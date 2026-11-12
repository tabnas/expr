/* Copyright (c) 2021-2025 Richard Rodger, MIT License */

// This algorithm is based on Pratt parsing, and draws heavily from
// the explanation written by Aleksey Kladov here:
// https://matklad.github.io/2020/04/13/simple-but-powerful-pratt-parsing.html
// See the `prattify` function for the core implementation.
//
// Expressions are encoded as LISP-style S-expressions using
// arrays. The operation meta data is provided as the first array
// element.  To maintain the integrity of the overall JSON AST,
// expression rules cannot simply re-assign nodes. Instead the
// existing partial expression nodes are rewritten in-place.
//
// Parentheses can have preceeding values, which allows for the using function
// call ("foo(1)") and index ("a[1]") syntax. See the tests for examples and
// configuration options.
//
// Ternary expressions are implemented as special rule that is similar to
// the parenthesis rule. You can have multiple ternaries.
//
// Standard Jsonic allows for implicit lists and maps (e.g. a,b =>
// ['a','b']) at the top level. This expression grammar also allows
// for implicits within parentheses, so that "foo(1,2)" =>
// ['(','foo',[1,2]]. To support implicits additional counters and
// flags are needed, as well as context-sensitive edge-case
// handling. See the ternary rule for a glorious example.
//
// There is a specific recurring edge-case: when expressions are the
// first item of a list, special care is need not to embed the list
// inside the expression.

// TODO: custom ctx.F for Op - make this automatic in options
// TODO: error on incomplete expr: 1+2+

// The engine is the tabnas parser; jsonic supplies the relaxed-JSON
// grammar. Engine types are re-exported by @tabnas/parser.
import {
  Tabnas,
  Plugin,
  Rule,
  RuleSpec,
  AltSpec,
  AltMatch,
  Tin,
  Context,
  Token,
  util,
} from '@tabnas/parser'

const { omap, entries, values } = util

// Operator definition (value of options.op map entry).
type OpDef = {
  src?: string | string[]
  osrc?: string
  csrc?: string
  left?: number
  right?: number
  use?: any // custom op data
  prefix?: boolean
  suffix?: boolean
  infix?: boolean
  ternary?: boolean
  paren?: boolean
  preval?: {
    active?: boolean
    required?: boolean
    allow?: string[]
  }

  // A paren operator's content. `required`: the content may not be empty,
  // so `()` is refused while a call that admits none stays `f()`.
  // `separated`: the content is a list of terms separated by commas, as a
  // call's arguments are, and each is a term of the operator
  // (`f(a, b)` is `[f, a, b]` under a call), so an infix operator on `,`
  // does not apply directly inside it.
  content?: {
    required?: boolean
    separated?: boolean
  }

  // An infix operator on `,` that gives way to the separator: wherever an
  // enclosing context separates on `,` (a list, a map, or a paren whose
  // content is separated) the comma separates, and only elsewhere is it
  // this operator. A paren whose content is not separated, a group, is
  // such an elsewhere, even inside a call.
  separator?: boolean

  // A CAST: a prefix operator whose token is a group (`(int)` in
  // `(int)a`) that `accept` accepts. `osrc` and `csrc` name the group,
  // which a paren operator of the table on the same opener and closer
  // parses; `right` is the cast's binding power, as a prefix's; `src`, by
  // default `osrc` and `csrc` together, is what `simplify` writes. The
  // operation's terms are the group's content and the operand. A group
  // `accept` refuses stays a group, or a call.
  cast?: boolean
  accept?: (term: any) => boolean

  // STRUCTURAL (key) operator: an occurrence is a map that owns the member
  // named by `src` (for example `$ref`), recognised by shape when a tree is
  // folded, never by a token. No fixed token is registered, so the base
  // grammar's pair rule keeps the key position, and no binding power
  // applies, since a key operator never competes for an operand: its two
  // terms are fixed by the shape, the member's value and the sibling map.
  key?: boolean

  // A key operator that a fold recognises and reports but never
  // evaluates: `$dynamicRef`, whose meaning depends on a scope the parser
  // does not have. Its map folds as a plain map, so operators inside it
  // still reduce. A host that wants to look at such a site declares it as
  // an ordinary key operator and returns `site.plain()` instead.
  dynamic?: boolean
}

// Options for the plugin.
type ExprOptions = {
  op?: { [name: string]: OpDef }

  // Reduce each expression to a value as it closes.
  evaluate?: Evaluate

  // Fold the finished document before `parse` returns (deferred mode):
  // the plugin subscribes to the engine's post-process rule event and
  // replaces the root node with `fold(root, options.fold)` on the last
  // pass of the parse. Key operators resolve against the finished root,
  // where `evaluate` above reduces a token expression at its own close.
  fold?: FoldOptions

  // Values side by side, with nothing between them, inside an expression
  // or a paren group make an implicit list, as jsonic makes one (`f(a b)`
  // is a call of `[a, b]`). With `false` they are refused, with the
  // engine's `unexpected`. A document's own implicit list, outside any
  // expression, and the items of a list or a map are unchanged.
  juxtapose?: boolean
}

// Full operator description (provided for evaluation).
type Op = {
  name: string
  src: string
  left: number
  right: number
  use: any
  prefix: boolean
  suffix: boolean
  infix: boolean
  ternary: boolean
  paren: boolean
  terms: number
  tkn: string
  tin: number
  osrc: string
  csrc: string
  otkn: string
  otin: number
  ctkn: string
  ctin: number
  preval: {
    active: boolean
    required: boolean
    allow?: string[]
  }
  content: {
    required: boolean
    separated: boolean
  }
  separator: boolean
  cast: boolean
  accept?: (term: any) => boolean
  key: boolean
  dynamic: boolean
  token: Token
  OP_MARK: typeof OP_MARK
}

// Lookup operators by token.
type OpMap = { [tin: number]: Op }

// Lookup key operators by the member name that marks a site, in the
// order a map that owns two of them is resolved by: static operators
// before dynamic ones, then member names in code-point order.
type KeyMap = Map<string, Op>

// Resolve the value of an operation: `terms` is one array, the folded
// terms in order.
type Evaluate = (rule: Rule, ctx: Context, op: Op, terms: any[]) => any

// A location, as segments from a document's root: member names, and
// array indices as numbers or as RFC 6901 index strings (`0`, `12`).
type Path = (string | number)[]

// What `site.at` gives back: whether the location names anything,
// whether reaching it closed a cycle, and, only when it was found and is
// not a cycle, its folded value.
type FoldHit = { found: boolean; cycle: boolean; value?: any }

// What `fold` hands its callbacks for one map (or one token operator
// array): the verbatim root of the map's document and that document's
// key, the path to the map, the verbatim map, the bag inherited along the
// path (`inherit`, applied at every map from the root down to and
// including this one), the map's opening token when positions are
// recorded, and the ways to reach the rest of the tree: `at` folds
// another location, in this document or another, through any key-operator
// sites on the way; `locate` reads one raw; `plain` is this map folded as
// a plain map, member by member in its own order, which is what a
// resolver returns where the operator does not apply.
type Site = {
  readonly root: any
  readonly doc: string
  readonly path: Path
  readonly node: any
  readonly k: Record<string, any>
  readonly token?: Token
  at: (path: Path, doc?: string) => FoldHit
  locate: (path: Path, doc?: string) => { found: boolean; value?: any }
  plain: () => any
}

// Reduce one site: a key operator with `[value, siblings]`, both already
// folded, or, when `tokens` is on, a token operator met in the walk with
// its folded terms.
type FoldEvaluate = (site: Site, op: Op, terms: any[]) => any

// The bag a map's subtree inherits, from the bag its parent's subtree
// sees. Called once per map on a path the fold needs, from the root
// down, so a site reached early through a reference gets ITS ancestors'
// context, never the referrer's: the engine's `k` discipline, for a
// finished tree. Inside the call, `site.k` is the parent's bag.
type Inherit = (site: Site, node: any, k: Record<string, any>) => Record<string, any>

// Whether a map that owns a key operator's member is an occurrence of it
// here. A map for which this is false folds as a plain map, with no
// `evaluate` call and no report row.
type Recognise = (site: Site, op: Op) => boolean

// The opening token of a container, by the container's identity.
type Positions = (node: object) => Token | undefined

// One report row: the site (`doc`, `at`) and the location it named
// (`targetDoc`, `target`), as document keys and RFC 6901 pointers.
type FoldEntry = { doc: string; at: string; targetDoc: string; target: string }

type FoldReport = {
  cycles: FoldEntry[]
  unresolved: FoldEntry[]
  dynamic: { doc: string; at: string; src: string }[]
}

type FoldOptions = {
  // The operator table; only the `key` entries matter to a fold. The
  // plugin supplies its own table in deferred mode.
  op?: { [name: string]: OpDef }
  evaluate: FoldEvaluate
  inherit?: Inherit
  recognise?: Recognise
  // The starting bag of every document's root.
  k?: Record<string, any>
  // The key of the document being folded. Default ''.
  doc?: string
  // Another document's raw root, by key, for `site.at(path, doc)`: called
  // once per key, `undefined` when it cannot be had. Deciding what a key
  // may name, and loading it, is the host's.
  load?: (doc: string, site: Site) => any
  // A cycle: tell the resolver (`cycle: true`), which is the default, or
  // throw a FoldError.
  cycle?: 'verbatim' | 'error'
  // Reduce token-operator arrays met in the walk through `evaluate` too,
  // so one fold handles `+` and `$ref`. Default true.
  tokens?: boolean
  // The opening token of each container, as a lookup by identity
  // (`recordPositions` builds one); in deferred mode `true` records them
  // during the parse.
  positions?: boolean | Positions
  // How deep the walk may go, nesting and reference hops counted
  // together. Default 1000.
  maxDepth?: number
  report?: (report: FoldReport) => void
}

// Mark Operator objects as owned by this plugin.
const OP_MARK = {}

// The plugin itself.
let Expr: Plugin = function Expr(tn: Tabnas, options: ExprOptions) {
  // Ensure comment matcher is first to avoid conflicts with
  // comment markers (//, /*, etc)
  // let lexm = jsonic.options.lex?.match || []
  // let cmI: number = lexm.map((m) => m.name).indexOf('makeCommentMatcher')
  // if (0 < cmI) {
  //   jsonic.options({
  //     lex: {
  //       match: [lexm[cmI], ...lexm.slice(0, cmI), ...lexm.slice(cmI + 1)],
  //     },
  //   })
  // }

  // console.log('EXPR', options)

  let token = tn.token.bind(tn) as any
  let fixed = tn.fixed.bind(tn) as any

  // Build token maps (TM).
  let optop = options.op || {}

  // Delete operations marked null.
  for (let opname in optop) {
    if (null === optop[opname]) {
      delete optop[opname]
    }
  }

  const prefixTM: OpMap = makeOpMap(token, fixed, optop, 'prefix')
  const suffixTM: OpMap = makeOpMap(token, fixed, optop, 'suffix')
  const infixTM: OpMap = makeOpMap(token, fixed, optop, 'infix')
  const ternaryTM: OpMap = makeOpMap(token, fixed, optop, 'ternary')

  const keyTM: KeyMap = makeKeyMap(optop)

  // The default grouping paren yields to the table's own: `plain`, as the
  // defaults define it, is left out when another paren operator opens
  // with `(` and closes with `)`. Otherwise it would be that opener's
  // group beside a call the table declares there, which a table that
  // gave its call the group's job too (one paren operator whose preval
  // is active and not required) never asked for. A `plain` the options
  // change is theirs, and stays.
  dropDefaultPlain(optop)

  // Several paren operators may share an opener: a group, whose preval is
  // inactive, and a call or a subscript, whose preval is active. A value
  // before the opener selects the preval one, and no value the group, so
  // `(e)` and `e()` are different operators. Each map gives, for an
  // opener, the operator of one of the two forms: the last declared of
  // its kind, and for the group, failing an operator whose preval is
  // inactive, the last whose preval is active and not required, which
  // serves both forms as a lone operator always has.
  const parenOps: Op[] = makeParenOps(token, fixed, optop)
  const parenPlainTM: OpMap = {}
  const parenPrevalTM: OpMap = {}
  for (const pop of parenOps) {
    if (pop.preval.active) {
      parenPrevalTM[pop.otin] = pop
    }
    const group = parenPlainTM[pop.otin]
    if (!pop.preval.active ||
      (!pop.preval.required && (null == group || group.preval.active))) {
      parenPlainTM[pop.otin] = pop
    }
  }

  let parenFixed = parenOps.reduce(
    (a, p) => ((a[p.otkn] = p.osrc), (a[p.ctkn] = p.csrc), a),
    {} as any,
  )

  // NOTE: operators with same src will generate same token - this is correct.
  let operatorFixed = Object.values({
    ...prefixTM,
    ...suffixTM,
    ...infixTM,
    ...ternaryTM,
  }).reduce((a, op) => ((a[op.tkn] = op.src), a), {} as any)

  tn.options({
    fixed: {
      token: { ...operatorFixed, ...parenFixed },
    },

    lex: {
      match: {
        comment: { order: 1e5 },
      },
    },
  })

  // Deferred mode: fold the finished root on the last pass of the parse.
  // The engine's post-process rule event fires after every pass in every
  // grammar; the last pass is a depth-0 close whose alternate neither
  // pushes, replaces nor failed (a rule with no close alternates reports
  // none), outside any probe and not synthesised by recovery. A parse
  // that fails is therefore never folded: the engine reports the failing
  // pass with `alt.err` set, from a handler that swallows subscriber
  // errors. The start rule's own after-close is not that event: a grammar
  // may replace its start rule (yaml's `stream` does on a directive or a
  // document marker), and `parse` returns the original root rule's node,
  // which is what is written here.
  if (options.fold) {
    const foldOptions = options.fold
    const deferred: FoldOptions = {
      ...foldOptions,
      op: optop,
      positions: true === foldOptions.positions ? recordPositions(tn) : foldOptions.positions,
    }
    tn.sub({
      ruleDone: (r: Rule, ctx: Context, done: any) => {
        if (0 !== r.d || 'c' !== done.state || done.probe || done.forced) return
        const alt = done.alt
        if (null != alt && (alt.p || alt.r || alt.err)) return
        // A token the last close backtracked over is still in the
        // lookahead buffer, and the engine's trailing-content check fails
        // the parse on it once the rule loop ends: such a parse is not
        // folded, so a fold error cannot stand in for the syntax error.
        // Under recovery the parse returns its value all the same.
        const t0 = ctx.t0
        if (!ctx.cfg.parse.recover.enabled && null != t0 &&
          ctx.NOTOKEN.tin !== t0.tin && ZZ !== t0.tin) return
        const root = ctx.root()
        root.node = foldTree(root.node, deferred, keyTM)
      },
    })
  }

  // Append 'expr' to the g (group tag) of every alt added below. Mirrors
  // the jsonic grammar(...) setting {rule:{alt:{g:'expr'}}}, applied
  // manually because the plugin uses jsonic.rule() (not jsonic.grammar()).
  const tagExpr = (alts: any): any => {
    if (!Array.isArray(alts)) return alts
    return alts.map((a) => {
      if (null == a || 'object' !== typeof a) return a
      const existing = null == a.g
        ? []
        : Array.isArray(a.g)
          ? [...a.g]
          : String(a.g).split(/\s*,\s*/).filter((s: string) => s.length > 0)
      return { ...a, g: [...existing, 'expr'] }
    })
  }

  const rule = (name: string, fn: (rs: RuleSpec) => void) => {
    tn.rule(name, (rs: RuleSpec) => {
      const origOpen = rs.open.bind(rs)
      const origClose = rs.close.bind(rs)
      ;(rs as any).open = (alts: any, flags?: any) => origOpen(tagExpr(alts), flags)
      ;(rs as any).close = (alts: any, flags?: any) => origClose(tagExpr(alts), flags)
      fn(rs)
    })
  }

  const PREFIX = values(prefixTM).map((op: any) => op.tin)
  const INFIX = values(infixTM).map((op: any) => op.tin)
  const SUFFIX = values(suffixTM).map((op: any) => op.tin)

  const TERN0 = values(ternaryTM)
    .filter((op: any) => 0 === op.use.ternary.opI)
    .map((op: any) => op.tin)
  const TERN1 = values(ternaryTM)
    .filter((op: any) => 1 === op.use.ternary.opI)
    .map((op: any) => op.tin)

  const OP = [...new Set(parenOps.map((pdef: Op) => pdef.otin as Tin))]
  const CP = [...new Set(parenOps.map((pdef: Op) => pdef.ctin as Tin))]

  // The paren operator a paren rule opens: the preval one when a value
  // precedes the opener, which the val that pushed this rule's expr
  // records as u.paren_preval, or when the paren is a postfix of the
  // expression its expr extends (u.expr_postfix), and the group otherwise.
  const parenOpFor = (r: Rule, tin: Tin): Op | undefined =>
    (r.parent?.parent?.u?.paren_preval || r.parent?.u?.expr_postfix
      ? parenPrevalTM : parenPlainTM)[tin]

  const hasRequired = parenOps.some((op: Op) => op.content.required)

  const hasPrefix = 0 < PREFIX.length
  const hasInfix = 0 < INFIX.length
  const hasSuffix = 0 < SUFFIX.length
  const hasTernary = 0 < TERN0.length && 0 < TERN1.length
  const hasParen = 0 < OP.length && 0 < CP.length

  const CA = tn.token.CA
  const CS = tn.token.CS
  const CB = tn.token.CB
  const TX = tn.token.TX
  const NR = tn.token.NR
  const ST = tn.token.ST
  const VL = tn.token.VL
  const ZZ = tn.token.ZZ

  const VAL = [TX, NR, ST, VL]

  const NONE = null as unknown as AltSpec

  // --- The options of #105, every one off unless a table asks ----------

  // Casts, by the opener of the group each is written with.
  const castTM: OpMap = makeCastOps(token, fixed, optop)
  const hasCast = 0 < Object.keys(castTM).length

  // Whether the paren this expr rule pushed closed the token of a cast: a
  // group (no value before it) on a cast's opener and closer, holding a
  // content the cast's `accept` accepts. A cast without `accept` accepts
  // nothing.
  const castHere = (r: Rule): boolean => {
    const child = r.child
    if (null == child || 'paren' !== child.name || !child.u.expr_paren_group) {
      return false
    }
    const node: any = child.node
    const group: Op | undefined = child.u.expr_paren_op
    const cast = null == group ? undefined : castTM[group.otin]
    return null != cast && null != cast.accept && cast.ctin === group?.ctin &&
      isOp(node) && 2 === node.length && !!cast.accept(node[1])
  }

  // A preval paren with a binding power, `left`, binds like a suffix: to
  // the value just before it only where it binds at least as tightly as
  // the operator whose operand that value is, and otherwise to the
  // expression that operator heads (`a.b[1]` is `(a.b)[1]` when `[` binds
  // no tighter than `.`).
  const bound = (op: Op | undefined): op is Op =>
    null != op && Number.MIN_SAFE_INTEGER !== op.left
  const hasBound = values(parenPrevalTM).some((op: Op) => bound(op))
  const postfixOpener = (tin: Tin) => bound(parenPrevalTM[tin])

  // The operator whose operand a val is, when its parent is an expr rule
  // building one: the operator the expr's node heads.
  const enclosing = (r: Rule): Op | undefined => {
    const parent = r.parent
    const node: any = parent?.node
    return null != parent && 'expr' === parent.name && isOp(node) &&
      !isTernaryOp(node) && !isParenOp(node) ? node[0] : undefined
  }

  // Whether a bound preval paren takes the value before it, compared as a
  // suffix is with the operator it would drill into. A prefix built into
  // an enclosing tree (u.expr_inner) leaves the val holding that tree, not
  // its own value, so the enclosing expression places the paren instead.
  const bindsHere = (r: Rule, op: Op): boolean => {
    if (!bound(op)) {
      return true
    }
    if (r.child?.u?.expr_inner) {
      return false
    }
    const encl = enclosing(r)
    return null == encl || encl.right <= op.left
  }

  // Separators. A comma separates in a paren whose content is separated,
  // and, once an infix operator on `,` is declared a separator, in lists
  // and maps too: jsonic's comma, or the token an infix operator on `,`
  // uses, which a host lexer may emit in its place. n.expr_sep marks a
  // context that separates.
  const commaInfix = values(infixTM).filter((op: Op) => ',' === op.src)
  const SEP = [...new Set([CA, ...commaInfix.map((op: Op) => op.tin as Tin)])]
  const hasSeparated = parenOps.some((op: Op) => op.content.separated)
  const hasSeparator = values(infixTM).some((op: Op) => op.separator)
  const separating = hasSeparated || hasSeparator
  const separatedHere = (r: Rule) => 0 < (r.n.expr_sep || 0)

  const openParen = makeOpenParen(parenOpFor, separating)

  // Values side by side. A token that starts a value, where the one before
  // has ended.
  const noJuxtapose = false === options.juxtapose
  const startsValue = (tin: Tin) =>
    VAL.includes(tin) || tn.token.OB === tin || tn.token.OS === tin ||
    OP.includes(tin) || PREFIX.includes(tin)

  rule('val', (rs: RuleSpec) => {
    // TODO: jsonic - make it easier to handle this case
    // Implicit pair not allowed inside ternary
    if (hasTernary && TERN1.includes(tn.token.CL)) {
      rs.def.open
        .filter((a: any) => a.g.includes('pair'))
        .map((alt: any) => {
          let origcond = alt.c
          let internary = (r: Rule) => !r.n.expr_ternary
          alt.c = origcond
            ? (r: Rule, ctx: Context) => origcond(r, ctx) && internary(r)
            : internary
        })
    }

    // With `juxtapose: false`, the base grammar's implicit list of values
    // side by side is refused inside a paren group, where the paren rule
    // allows implicits again, and after a value an expression made (its
    // val pushed an expr rule: `-a b`): no alternate takes the second
    // value, and the parse fails on it with `unexpected`.
    if (noJuxtapose) {
      rs.def.close
        .filter((a: any) => a.g.includes('imp') && a.g.includes('space') &&
          a.g.includes('jsonic'))
        .map((alt: any) => {
          let origcond = alt.c
          let plain = (r: Rule) =>
            !(0 < (r.n.expr_paren || 0)) && 'expr' !== r.child?.name
          alt.c = origcond
            ? (r: Rule, ctx: Context) => origcond(r, ctx) && plain(r)
            : plain
        })
    }

    rs.open([
      // The prefix operator of the first term of an expression.
      hasPrefix
        ? {
          s: [PREFIX],
          b: 1,
          n: { expr_prefix: 1, expr_suffix: 0 },
          p: 'expr',
          g: 'expr,expr-prefix',
        }
        : NONE,

      // WWW
      hasParen
        ? {
          s: [VAL, OP],
          b: 1,
          p: 'expr',
          c: (r: Rule, ctx: Context) => {
            const pdef = parenPrevalTM[r.o1.tin]
            const preval = r.o0.resolveVal(r, ctx)
            return null != pdef &&
              (null == pdef.preval.allow || pdef.preval.allow.includes(preval)) &&
              bindsHere(r, pdef)
          },
          u: { paren_preval: true },
          g: 'expr,expr-paren,expr-paren-preval',
          a: (r: Rule, ctx: Context) => {
            r.node = r.o0.resolveVal(r, ctx)
          }
        }
        : NONE,

      // An opening parenthesis.
      // NOTE: this can happen outside an expression.
      hasParen
        ? {
          s: [OP],
          b: 1,
          p: 'expr',

          c: (r: Rule) => null != parenPlainTM[r.o0.tin],

          g: 'expr,expr-paren',
        }
        : NONE,
    ]).close([
      // A separator where the context separates (n.expr_sep): the value
      // ends, and the rule that collects the terms takes the comma.
      separating
        ? {
          s: [SEP],
          c: separatedHere,
          b: 1,
          g: 'expr,expr-sep',
        }
        : NONE,

      // Comma-op suppression. When a parent rule (e.g. an embedding
      // grammar's wrapper rule) sets n.no_comma_op, bail at `,` without
      // treating it as the comma operator — the parent then consumes
      // the `,` itself as a separator. Match by `src` on the next infix
      // token so this works regardless of which token the embedding
      // grammar uses for `,`.
      hasInfix
        ? {
          s: [INFIX],
          c: (r: Rule) =>
            0 < (r.n.no_comma_op || 0) && r.c0?.src === ',',
          b: 1,
          g: 'expr,no-comma-op-bail',
        }
        : NONE,

      hasTernary
        ? {
          s: [TERN0],
          c: (r: Rule) => !r.n.expr,
          b: 1,
          r: 'ternary',
          g: 'expr,expr-ternary',
        }
        : NONE,

      // The infix operator following the first term of an expression.
      hasInfix
        ? {
          s: [INFIX],
          b: 1,
          n: { expr_prefix: 0, expr_suffix: 0 },
          r: (r: Rule) => (!r.n.expr ? 'expr' : ''),
          g: 'expr,expr-infix',
        }
        : NONE,

      // The suffix operator following the first term of an expression.
      hasSuffix
        ? {
          s: [SUFFIX],
          b: 1,
          n: { expr_prefix: 0, expr_suffix: 1 },
          r: (r: Rule) => (!r.n.expr ? 'expr' : ''),
          g: 'expr,expr-suffix',
        }
        : NONE,

      // The closing parenthesis of an expression.
      hasParen
        ? {
          s: [CP],
          c: (r: Rule) => !!r.n.expr_paren,
          b: 1,
          g: 'expr,expr-paren',
        }
        : NONE,

      // Chain for postfix paren forms. When a val has just produced a
      // value (e.g. `a[0]`, `f(0)`, or a parenthesised expression like
      // `(*p)`) and the next token is another preval-active paren-open,
      // push expr (which descends into paren) so the new paren-form
      // picks up this val's node as the preval. Use `p: 'expr'` (not
      // `r: 'val'`) so the current val rule stays alive and
      // `ctx.root().node` still reflects the chained result on parser
      // return. `u: { paren_preval: true }` is set so makeCloseParen
      // finds it on r.parent.parent (= this val) and pushes our node
      // into the new paren CST. This complements the open-time
      // s:[VAL,OP] preval alt above, which only fires on the first
      // preval-paren of an expression; chain-time detection is needed
      // for subsequent parens because the leading "value" is by then a
      // produced node, not a token in the lex buffer.
      hasParen
        ? {
          s: [OP],
          b: 1,
          c: (r: Rule) => {
            const pdef = parenPrevalTM[r.c0.tin]
            return null != pdef &&
              undefined !== r.node &&
              (null == pdef.preval.allow || pdef.preval.allow.includes(r.node)) &&
              bindsHere(r, pdef)
          },
          p: 'expr',
          u: { paren_preval: true },
          g: 'expr,expr-paren,expr-paren-preval-chain',
        }
        : NONE,

      // A bound preval paren that binds looser than the operator whose
      // operand this value is: the value ends, and the expression that
      // operator heads takes the paren as its postfix.
      hasBound
        ? {
          s: [OP],
          b: 1,
          c: (r: Rule) => {
            const pdef = parenPrevalTM[r.c0.tin]
            return bound(pdef) && undefined !== r.node && !bindsHere(r, pdef)
          },
          g: 'expr,expr-paren,expr-postfix',
        }
        : NONE,

      hasTernary
        ? {
          s: [TERN1],
          c: (r: Rule) => !!r.n.expr_ternary,
          b: 1,
          g: 'expr,expr-ternary',
        }
        : NONE,

      // Don't create implicit list inside expression (comma separator).
      {
        s: [CA],
        c: (r: Rule) =>
          (1 === r.d && (1 <= r.n.expr || 1 <= r.n.expr_ternary)) ||
          (1 <= r.n.expr_ternary && 1 <= r.n.expr_paren),
        b: 1,
        g: 'expr,list,val,imp,comma,top',
      },

      // Don't create implicit list inside expression (space separator).
      {
        s: [VAL],
        c: (r: Rule) =>
          (1 === r.d && (1 <= r.n.expr || 1 <= r.n.expr_ternary)) ||
          (1 <= r.n.expr_ternary && 1 <= r.n.expr_paren),
        b: 1,
        g: 'expr,list,val,imp,space,top',
      },
    ])
  })


  rule('list', (rs: RuleSpec) => {
    // rs.bo(false, (...rest: any) => {
    rs.bo(false, (r: Rule) => {
      // List elements are new expressions.
      // Unless this is an implicit list.
      if (!r.prev.u.implist) {
        r.n.expr = 0
        r.n.expr_prefix = 0
        r.n.expr_suffix = 0
        r.n.expr_paren = 0
        r.n.expr_ternary = 0

        // A list separates on `,` once an infix operator on `,` gives way
        // to separators, and otherwise leaves the comma to that operator.
        if (separating) {
          r.n.expr_sep = hasSeparator ? 1 : 0
        }
      }
    }).close([
      hasParen && {
        s: [CP],

        // If end of normal list, consume `]` - it's not a close paren.
        b: (r: Rule) => (CS === r.c0.tin && !r.n.expr_paren ? 0 : 1),
      },

      // An implicit list of values side by side ends at a separator of
      // the paren content it is a term of.
      separating
        ? {
          s: [SEP],
          c: (r: Rule) => separatedHere(r) && !!r.prev?.u?.implist,
          b: 1,
          g: 'expr,expr-sep',
        }
        : NONE,
    ])
  })

  rule('map', (rs: RuleSpec) => {
    rs.bo(false, (...rest: any) => {
      // Map values are new expressions.
      rest[0].n.expr = 0
      rest[0].n.expr_prefix = 0
      rest[0].n.expr_suffix = 0
      rest[0].n.expr_paren = 0
      rest[0].n.expr_ternary = 0

      // A map separates on `,` as a list does.
      if (separating) {
        rest[0].n.expr_sep = hasSeparator ? 1 : 0
      }
    }).close([
      hasParen && {
        s: [CP],
        // If end of normal map, consume `}` - it's not a close paren.
        b: (r: Rule) => (CB === r.c0.tin && !r.n.expr_paren ? 0 : 1),
      },
    ])
  })

  rule('elem', (rs: RuleSpec) => {
    rs.close([
      // An implicit list of values side by side ends at a separator of
      // the paren content it is a term of: the list ends with it.
      separating
        ? {
          s: [SEP],
          c: (r: Rule) => separatedHere(r) && !!r.parent?.prev?.u?.implist,
          b: 1,
          g: 'expr,expr-sep',
        }
        : NONE,

      // Close implicit list within parens.
      hasParen
        ? {
          s: [CP],
          b: 1,
          c: (r: Rule) => !!r.n.expr_paren,
          g: 'expr,expr-paren,imp,close,list',
        }
        : NONE,

      // Following elem is a paren expression.
      hasParen
        ? {
          s: [OP],
          b: 1,
          r: 'elem',
          g: 'expr,expr-paren,imp,open,list',
        }
        : NONE,
    ])
  })

  rule('pair', (rs: RuleSpec) => {
    rs.close([
      // Close implicit map within parens.
      hasParen
        ? {
          s: [CP],
          b: 1,
          c: (r: Rule) => !!r.n.expr_paren || 0 < r.n.pk,
          g: 'expr,expr-paren,imp,map',
        }
        : NONE,
    ])
  })

  rule('expr', (rs: RuleSpec) => {
    rs.open([

      // A cast: the group just closed (the expr this one replaced holds
      // it, `[group, type]`) heads a prefix expression, `[cast, type]`,
      // whose operand follows. It takes no token: the group was the
      // token.
      hasCast
        ? {
          c: (r: Rule) => !!r.prev?.u?.expr_cast_next,
          n: { expr: 1, expr_prefix: 1, expr_suffix: 0, dlist: 1, dmap: 1 },
          p: 'val',
          a: (r: Rule) => {
            const group: any = r.prev.node
            const cast = castTM[group[0].otin]
            const op: Op = { ...cast, token: group[0].token, OP_MARK }
            r.node = prior(r, r.parent, op, 'expr-cast')
            r.node.push(group[1])

            // The val that pushed the group's expr reads its node from
            // that expr, its child, when it closes: that expr holds the
            // cast too.
            r.prev.node = r.node
          },
          g: 'expr,expr-cast',
        }
        : NONE,

      // A bound preval paren after an expression: a postfix operator that
      // takes the expression, or as much of it as it binds tighter than,
      // as its first term, placed as a suffix is (prattifyPostfix). The
      // paren then parses the content into the same node.
      hasBound
        ? {
          s: [OP],
          c: (r: Rule) => !!r.prev?.u?.expr_postfix_next,
          n: { expr: 1, expr_prefix: 0, dlist: 1, dmap: 1 },
          p: 'paren',
          b: 1,
          a: (r: Rule) => {
            const op = makeOp(r.o0, parenPrevalTM)
            const node = isOp(r.prev.node)
              ? prattifyPostfix(r.prev.node, op)
              : prior(r, r.prev, op, 'expr-postfix')
            r.node = node
            r.u.expr_postfix = node
          },
          g: 'expr,expr-paren,expr-postfix',
        }
        : NONE,

      // An opening parenthesis of an expression.
      hasParen
        ? {
          s: [OP],
          p: 'paren',
          b: 1,

          g: 'expr,expr-paren,expr-start',
        }
        : NONE,

      hasPrefix
        ? {
          s: [PREFIX],
          c: (r: Rule) => !!r.n.expr_prefix,
          n: { expr: 1, dlist: 1, dmap: 1 },
          p: 'val',
          g: 'expr,expr-prefix',
          a: (r: Rule) => {
            const op = makeOp(r.o0, prefixTM)
            // Only fold into the parent's node when it is an expression
            // op THIS prefix chain is building (e.g. the outer `-` of
            // `--1`). A ternary/paren op-array on the parent is a foreign
            // seed the engine threaded down by parent-seeding — prattify
            // would splice the new prefix into the wrapper (corrupting it
            // with a stray hole + duplicated operand). Start a fresh
            // operand expression for those (mirrors the infix guard).
            if (isOp(r.parent.node)
              && !isTernaryOp(r.parent.node)
              && !isParenOp(r.parent.node)) {
              r.node = prattify(r.parent.node, op, 'expr-prefix')

              // This prefix is a term of the expression the parent is
              // building, inside that expression's own tree, so this rule
              // does not evaluate it: the expression that holds the root
              // evaluates the whole tree once it ends (see the after-close
              // below).
              r.u.expr_inner = true
            }
            else {
              r.node = prior(r, r.parent, op, 'expr-prefix')
            }
          },
        }
        : NONE,

      hasInfix
        ? {
          s: [INFIX],
          p: 'val',
          n: { expr: 1, expr_prefix: 0, dlist: 1, dmap: 1 },
          a: (r: Rule) => {
            const prev = r.prev
            const parent = r.parent
            const op = makeOp(r.o0, infixTM)

            // Second and further operators.
            if (isOp(parent.node) && !isTernaryOp(parent.node)) {
              // console.log('INFIX-A', r.i, p(r.node), parent.i, p(parent.node))
              r.node = prattify(parent.node, op, 'expr-infix-more')
              // console.log('INFIX-B', r.i, p(r.node), parent.i, p(parent.node))
            }

            // First term was unary expression.
            else if (isOp(prev.node)) {
              r.node = prattify(prev.node, op, 'expr-infix-unary')
              r.parent = prev
            }

            // First term was plain value or ternary part.
            else {
              r.node = prior(r, prev, op, 'expr-infix')
            }
          },
          g: 'expr,expr-infix',
        }
        : NONE,

      hasSuffix
        ? {
          s: [SUFFIX],
          n: { expr: 1, expr_prefix: 0, dlist: 1, dmap: 1 },
          a: (r: Rule) => {
            const prev = r.prev
            const op = makeOp(r.o0, suffixTM)
            if (isOp(prev.node)) {
              r.node = prattify(prev.node, op, 'expr-suffix')

              // The suffix took over the val of the expression's first
              // term, whose node holds the root (`-1!`, `(1)!`): the root
              // is evaluated from there, as the infix after a unary
              // expression does. A suffix that took over an expr rule
              // already has the first term's val as its parent.
              if ('val' === prev.name) {
                r.parent = prev
              }
            }
            else {
              r.node = prior(r, prev, op, 'expr-suffix')
            }
          },
          g: 'expr,expr-suffix',
        }
        : NONE,
    ])

      .bc((r: Rule) => {
        const addterm =
          isOp(r.node)
          && r.node?.length - 1 < r.node[0].terms
          && ('object' !== typeof r.node || r.node !== r.child.node)

        // Append final term to expression.
        if (addterm) {
          r.node.push(r.child.node)
        }
      })

      .close([
        // A group the table's cast accepts: the expression goes on as a
        // cast whose operand follows (the open alternate above), so this
        // close keeps n.expr and nothing evaluates the group yet.
        hasCast
          ? {
            c: (r: Rule) => castHere(r),
            // Inside a list, a map or a paren, n.expr is zero by descent,
            // and the after-close would evaluate the group.
            n: { expr: 1 },
            r: 'expr',
            u: { expr_cast_next: true },
            g: 'expr,expr-cast',
          }
          : NONE,

        // A postfix paren's expression goes on after the paren, as a
        // suffix's does, so it takes the alternates below.
        {
          c: (r: Rule) => 'paren' === r.child.name && !r.u.expr_postfix,
          n: { expr: 0 },
          g: 'expr,expr-end,expr-paren-end',
        },

        // Comma-op suppression. When n.no_comma_op is set by a parent
        // rule, terminate the expression at `,` without treating it as
        // the comma operator (mirrors the val.close bail above for
        // expressions that have already passed the first term). Match
        // by `src` on the next infix token. n: { expr: 0 } closes the
        // expression frame so the parent rule regains control before
        // any comma-op INFIX alt below fires.
        hasInfix
          ? {
            s: [INFIX],
            c: (r: Rule) =>
              0 < (r.n.no_comma_op || 0) && r.c0?.src === ',',
            b: 1,
            n: { expr: 0 },
            g: 'expr,no-comma-op-bail',
          }
          : NONE,

        // A separator where the context separates: the expression ends,
        // and the rule that collects the terms takes the comma.
        separating
          ? {
            s: [SEP],
            c: separatedHere,
            b: 1,
            n: { expr: 0 },
            g: 'expr,expr-sep',
          }
          : NONE,

        hasInfix
          ? {
            s: [INFIX],
            // Complete prefix first.
            c: (r: Rule) => !r.n.expr_prefix,
            b: 1,
            r: 'expr',
            g: 'expr,expr-infix,expr-prefix',
          }
          : NONE,

        // TTT
        hasInfix
          ? {
            s: [INFIX],
            // Complete prefix first.
            c: (r: Rule) => !!r.n.expr_prefix,
            b: 1,
            // r: 'expr',
            g: 'expr,expr-infix',
          }
          : NONE,


        hasSuffix
          ? {
            s: [SUFFIX],
            c: (r: Rule) => !r.n.expr_prefix,
            b: 1,
            r: 'expr',
            g: 'expr,expr-suffix,expr-prefix',
          }
          : NONE,

        // A suffix after a prefix expression: complete the prefix first,
        // as the infix alternate above does, and leave the suffix to the
        // expression the prefix belongs to, which drills it into the
        // prefix (`-1!` is `-(1!)`). The expression goes on, so this close
        // keeps n.expr, and nothing evaluates it here: the after-close
        // below evaluates an expression only once it has ended, and the
        // expression the suffix extends evaluates the whole tree.
        hasSuffix
          ? {
            s: [SUFFIX],
            c: (r: Rule) => !!r.n.expr_prefix,
            b: 1,
            g: 'expr,expr-suffix,expr-prefix',
          }
          : NONE,

        // A bound preval paren after this expression: a postfix that the
        // next expr rule places, as the suffix alternate above does.
        hasBound
          ? {
            s: [OP],
            c: (r: Rule) => postfixOpener(r.c0.tin) && !r.n.expr_prefix,
            b: 1,
            r: 'expr',
            u: { expr_postfix_next: true },
            g: 'expr,expr-paren,expr-postfix',
          }
          : NONE,

        // After a prefix expression: complete the prefix first, as for a
        // suffix, and leave the paren to the expression it belongs to.
        hasBound
          ? {
            s: [OP],
            c: (r: Rule) => postfixOpener(r.c0.tin) && !!r.n.expr_prefix,
            b: 1,
            g: 'expr,expr-paren,expr-postfix,expr-prefix',
          }
          : NONE,

        hasParen
          ? {
            s: [CP],
            c: (r: Rule) => !!r.n.expr_paren,
            b: 1,
          }
          : NONE,

        hasTernary
          ? {
            s: [TERN0],
            c: (r: Rule) => !r.n.expr_prefix,
            b: 1,
            r: 'ternary',
            g: 'expr,expr-ternary',
          }
          : NONE,

        // Implicit list at the top level.
        {
          s: [CA],
          // c: { d: 0 },
          c: (r: Rule) => r.d <= 0,
          n: { expr: 0 },
          r: 'elem',
          a: (r: Rule, ctx: Context) => (r.node = topList(r, ctx)),
          g: 'expr,comma,list,top',
        },

        // Implicit list at the top level.
        {
          s: [VAL],
          // c: { d: 0 },
          c: (r: Rule) => r.d <= 0 && !noJuxtapose,
          n: { expr: 0 },
          b: 1,
          r: 'elem',
          a: (r: Rule, ctx: Context) => (r.node = topList(r, ctx)),
          g: 'expr,space,list,top',
        },

        // Implicit list indicated by comma.
        {
          s: [CA],
          c: (r: Rule) => r.lte('pk'),
          n: { expr: 0 },
          b: 1,
          h: implicitList,
          g: 'expr,list,val,imp,comma',
        },

        // Implicit list indicated by space separated value.
        //
        // This alternative carries no token filter, so it also has to be
        // kept off a pending suffix operator. A suffix belongs to the
        // expression that just closed, not to a new element: `f(@x!)`
        // reached here on the `!` of a completed prefix, opened `elem`,
        // and offered `!` as the start of a value — which nothing matches,
        // so a valid expression failed as `unexpected`. Leaving the token
        // alone lets the val-close suffix alternative take it, which is
        // how the same expression already parses at the top level.
        {
          c: (r: Rule, ctx: Context) =>
            r.lte('pk') && r.lte('expr_suffix') &&
            !SUFFIX.includes(ctx.t0.tin) &&
            !(noJuxtapose && startsValue(ctx.t0.tin)),
          n: { expr: 0 },
          h: implicitList,
          g: 'expr,list,val,imp,space',
        },

        // Expression ends on non-expression token.
        {
          n: { expr: 0 },
          g: 'expr,expr-end',
        },
      ])

      .ac((r: Rule, ctx: Context) => {
        // Only evaluate at root of expr (where r.n.expr === 0)

        // console.log('EXPR-AC', r.name, r.i, r.n, p(r.node),
        //   'P', r.parent.name, r.parent.i, r.parent.n, p(r.parent.node))

        if (options.evaluate && 0 === r.n.expr) {
          // Evaluation follows the tree the S-expression shows, so an
          // expression is evaluated once, whole, from its root, once it
          // has ended. A prefix inside an operand of an enclosing
          // expression (`-2` in `1+-2`) was built into that expression's
          // tree: evaluating here evaluated the enclosing tree too, before
          // it was complete, and then again when it was.
          if (r.u.expr_inner) {
            return
          }

          const parentNode: any = r.parent.node

          // The parent holds an implicit list (`f(1+2, 3)`, `1+2, 3`)
          // rather than this expression's own op-array. A list is a plain
          // array, not an op-array, so its members used to reach the
          // caller as raw op-arrays that `evaluate` was never called on.
          //
          // Reduce them in place, keeping the array: the list is still
          // being collected, and the paren rule and the expr rules
          // between it and here all hold a reference to this exact array,
          // so replacing it would strand the members still to come. A
          // member already reduced is returned untouched rather than
          // reduced again.
          if (Array.isArray(parentNode) && !isOp(parentNode)) {
            for (let termI = 0; termI < parentNode.length; termI++) {
              parentNode[termI] = evaluation(
                r.parent,
                ctx,
                parentNode[termI],
                options.evaluate,
              )
            }

            r.node = parentNode

            return
          }

          // The parent node will contain the root of the expr tree.
          let out = evaluation(
            r.parent,
            ctx,
            r.parent.node,
            options.evaluate,
          )

          // Not onto the NORULE sentinel, for the reason `topList` gives:
          // NORULE carries a `node` like any other rule and a rule seeds
          // its node from its parent, so a value written there is handed
          // straight back out as the starting node of the rules that
          // follow. The Go port guards the same write; this runtime was
          // left unguarded when that fix landed. `r.node` below is what
          // the parse reads at this depth, so skipping the write costs
          // nothing.
          if (r.parent && ctx.NORULE !== r.parent) {
            r.parent.node = out
          }

          // A cast took over its group's expr rule, which the val that
          // pushed it reads its node from when it closes.
          if (r.prev?.u?.expr_cast_next) {
            r.prev.node = out
          }

          // Also write the evaluated result onto this expr rule's own
          // node. When the expr rule was PUSHED from a still-open val
          // (the prefix/paren forms: `+1`, `(a)`), that val's close
          // coalescing reads its child's node (this expr rule). Under
          // the new builtins val-close coalesces child-container over
          // the seeded node, which would otherwise restore the raw
          // op-array and discard the evaluated value. Writing `out`
          // here makes the coalesced value the evaluated result (a
          // primitive scalar or a real container), not the op-array.
          r.node = out
        }
      })
  })

  rule('paren', (rs: RuleSpec) => {
    rs.bo((r: Rule) => {
      // Allow implicits inside parens
      r.n.dmap = 0
      r.n.dlist = 0
      r.n.pk = 0
    })
      .open([
        // A paren whose content is required may not be empty: `()` is
        // refused at its closer, where a content was due.
        hasRequired
          ? {
            s: [OP, CP],
            c: (r: Rule) => {
              const op = parenOpFor(r, r.o0.tin)
              return op?.ctin === r.o1.tin && op.content.required
            },
            e: (r: Rule) => r.o1,
            g: 'expr,expr-paren,empty,required',
          }
          : NONE,

        hasParen
          ? {
            s: [OP, CP],
            b: 1,
            g: 'expr,expr-paren,empty',
            c: (r: Rule) => parenOpFor(r, r.o0.tin)?.ctin === r.o1.tin,
            a: openParen,
          }
          : NONE,

        hasParen
          ? {
            s: [OP],
            p: 'val',
            n: {
              expr_paren: 1,
              expr: 0,
              expr_prefix: 0,
              expr_suffix: 0,
            },
            g: 'expr,expr-paren,open',
            a: openParen,
          }
          : NONE,
      ])

      .close([
        // A separator in a separated content: the term before it is done,
        // and the next one follows. The loop is this rule's own close, so
        // a long argument list adds no rule depth. A term that is empty
        // (`f(,a)`, `f(a,,b)`) is refused at the separator after it.
        separating
          ? {
            s: [SEP],
            c: (r: Rule) => !!r.u.expr_paren_op?.content.separated &&
              undefined === parenTerm(r),
            e: (r: Rule) => r.c0,
            g: 'expr,expr-paren,expr-sep,empty',
          }
          : NONE,

        separating
          ? {
            s: [SEP],
            c: (r: Rule) => !!r.u.expr_paren_op?.content.separated,
            p: 'val',
            n: { expr: 0, expr_prefix: 0, expr_suffix: 0 },
            a: (r: Rule) => {
              ; (r.u.expr_items ??= []).push(parenTerm(r))
              // The next term starts empty, as the first did: a value an
              // earlier term wrote here (a ternary's) is not the next one.
              r.node = undefined
              r.u.expr_paren_more = true
            },
            g: 'expr,expr-paren,expr-sep',
          }
          : NONE,

        hasParen
          ? {
            s: [CP],
            // The closer of the operator this paren opened.
            c: (r: Rule) => r.u.expr_paren_op?.ctin === r.c0.tin,

            a: makeCloseParen,
            g: 'expr,expr-paren,close',
          }
          : NONE,
      ])

      .ac((r: Rule, _ctx: Context) => {
        // A separator's pass goes on to the next term: the paren is not
        // done.
        if (r.u.expr_paren_more) {
          r.u.expr_paren_more = false
          return
        }

        // A postfix paren extended the node of the expression it applies
        // to, in place, and its expr rule holds that node already: the
        // rules above hold the expression's root, which stays.
        if (r.parent.u.expr_postfix) {
          return
        }
        r.parent.node = r.node
        r.parent.parent.node = r.node
      })
  })

  // Ternary operators are like fancy parens.
  if (hasTernary) {
    rule('ternary', (rs: RuleSpec) => {
      rs.open([
        {
          s: [TERN0],
          p: 'val',
          n: {
            expr_ternary: 1,
            expr: 0,
            expr_prefix: 0,
            expr_suffix: 0,
          },
          u: { expr_ternary_step: 1 },
          g: 'expr,expr-ternary,open',
          a: (r: Rule) => {
            let op = makeOp(r.o0, ternaryTM)
            r.u.expr_ternary_name = op.name

            if (isOp(r.prev.node)) {
              r.node = updateExprNode(r.prev.node, op, dupNode(r.prev.node))
            } else {
              r.node = r.prev.node = updateExprNode([], op, r.prev.node)
            }

            r.u.expr_ternary_paren =
              r.n.expr_paren || r.prev.u.expr_ternary_paren || 0

            r.n.expr_paren = 0
          },
        },
        {
          p: 'val',
          c: (r: Rule) => 2 === r.prev.u.expr_ternary_step,
          a: (r: Rule) => {
            r.u.expr_ternary_step = r.prev.u.expr_ternary_step
            r.n.expr_paren = r.u.expr_ternary_paren =
              r.prev.u.expr_ternary_paren
          },
          g: 'expr,expr-ternary,step',
        },
      ]).close([
        {
          s: [TERN1],
          c: (r: Rule) => {
            return (
              1 === r.u.expr_ternary_step &&
              r.u.expr_ternary_name === ternaryTM[r.c0.tin].name
            )
          },
          r: 'ternary',
          a: (r: Rule) => {
            r.u.expr_ternary_step++
            r.node.push(r.child.node)
          },
          g: 'expr,expr-ternary,step',
        },

        // End of ternary at top level. Implicit list indicated by comma.
        {
          s: [[CA, ...CP]],
          // Not at a separator of a separated content, which ends the
          // ternary as one of that content's terms.
          c: (r: Rule) => implicitTernaryCond(r) &&
            !(separatedHere(r) && SEP.includes(r.c0.tin)),
          // Handle ternary as first item of imp list inside paren.
          b: (_r: Rule, ctx: Context) => (CP.includes(ctx.t0.tin) ? 1 : 0),
          r: (r: Rule, ctx: Context) =>
            !CP.includes(ctx.t0.tin) &&
              (0 === r.d ||
                (r.prev.u.expr_ternary_paren && !r.parent.node?.length))
              ? 'elem'
              : '',
          a: implicitTernaryAction,
          g: 'expr,expr-ternary,list,val,imp,comma',
        },

        // End of ternary at top level.
        // Implicit list indicated by space separated value.
        {
          // Neither at a separator of a separated content, nor, with
          // `juxtapose: false`, at a value beside the ternary.
          c: (r: Rule, ctx: Context) => implicitTernaryCond(r) &&
            !(separatedHere(r) && SEP.includes(ctx.t0.tin)) &&
            !(noJuxtapose && startsValue(ctx.t0.tin)),
          // Handle ternary as first item of imp list inside paren.
          r: (r: Rule, ctx: Context) => {
            return (0 === r.d ||
              !CP.includes(ctx.t0.tin) ||
              r.prev.u.expr_ternary_paren) &&
              !r.parent.node?.length &&
              ZZ !== ctx.t0.tin
              ? 'elem'
              : ''
          },
          a: implicitTernaryAction,
          g: 'expr,expr-ternary,list,val,imp,space',
        },

        // End of ternary.
        {
          c: (r: Rule) => 0 < r.d && 2 === r.u.expr_ternary_step,
          a: (r: Rule) => {
            r.node.push(r.child.node)
          },
          g: 'expr,expr-ternary,close',
        },
      ])
        // Ensure ternary results get evaluated. Without this, ternaries
        // that aren't wrapped in expr (e.g. val.close's TERN0 alt does
        // `r: 'ternary'` directly, not via an expr intermediate) leave
        // the result as a raw [op_ternary, ...] op-array. Fire on every
        // ternary instance's after-close, but only act when the chain
        // has reached its final step (the node has accumulated all 3
        // operands and r.next is not another ternary). Walk the
        // r.prev chain back to the original rule (the val that started
        // `r: 'ternary'`) and write the evaluated CST so jsonic returns
        // the structured conditional_expression instead of the op-array.
        .ac((r: Rule, ctx: Context) => {
          if (!options.evaluate) return
          // Skip while the chain is still ongoing: r:'ternary' replaces
          // the current rule with another ternary instance, and we want
          // to evaluate only on the FINAL step. r.next is the rule the
          // parser will process after this .ac returns.
          if (r.next && r.next.name === 'ternary') return
          if (!Array.isArray(r.node)) return
          if (!isOp(r.node)) return
          // Op-array isn't fully populated until 3 operands (cond, then,
          // else) sit at r.node[1..3]. Early steps have length 2 or 3.
          if (r.node.length < 4) return
          const out = evaluation(r, ctx, r.node, options.evaluate)
          // Write the evaluated CST back to every rule along the
          // r:-replacement chain (each successive ternary instance and
          // the original val that started the chain). Their .node
          // references currently all point at the same op-array;
          // replace them all with the structured CST so ctx.root().node
          // — whichever one jsonic returns — reflects the evaluated form.
          let cur: any = r
          while (cur) {
            cur.node = out
            cur = cur.prev
          }
          if (r.parent) r.parent.node = out
        })
    })
  }
}

// Convert prior (parent or previous) rule node into an expression.
function prior(rule: Rule, prior: Rule, op: Op, whence: string) {
  // console.log('PRIOR', whence, rule.i, p(rule.node), 'PR', prior.i, p(prior.node))

  let prior_node = prior.node
  if (isOp(prior.node) && !op.prefix) {
    // Infix/suffix: the prior node is the genuine first term to wrap
    // (e.g. the `(1+2)` of `(1+2)+3`). Duplicate it so the in-place
    // rewrite below preserves referential integrity.
    prior_node = dupNode(prior.node)
  } else {
    // A prefix op never has a prior term, so any op-array sitting on
    // prior.node here is a foreign parent-seed (a ternary/paren wrapper
    // threaded down by the engine). Start fresh rather than mutating it.
    prior.node = []
  }

  updateExprNode(prior.node, op)

  if (!op.prefix) {
    prior.node[1] = prior_node
  }

  // Ensure first term val rule contains final expression.
  rule.parent = prior

  return prior.node
}


// Add token so that expression evaluator can reference source locations.
function makeOp(t: Token, om: OpMap): Op {
  return { ...om[t.tin], token: t, OP_MARK }
}


function updateExprNode(node: any, op: Op, ...terms: any): any {
  let out = node
  out[0] = op

  let tI = 0
  for (; tI < terms.length; tI++) {
    out[tI + 1] = terms[tI]
  }
  out.length = tI + 1

  return out
}

function dupNode(node: any): any {
  let out: any = [...node]
  return out
}

function makeOpenParen(
  parenOpFor: (r: Rule, tin: Tin) => Op | undefined,
  separating: boolean,
) {
  return function openParen(r: Rule) {
    const op = parenOpFor(r, r.o0.tin) as Op
    // The operator this paren opened, which its close builds the node
    // with: the closer alone cannot say which, since two operators may
    // share one.
    r.u.expr_paren_op = op
    // A group, opened with no value before it, which a cast may take as
    // its token.
    r.u.expr_paren_group =
      !(r.parent?.parent?.u?.paren_preval || r.parent?.u?.expr_postfix)
    // The content separates on commas only in a paren whose content is
    // separated: a group inside a call takes the comma operator again.
    if (separating) {
      r.n.expr_sep = op.content.separated ? 1 : 0
    }
    let pd = 'expr_paren_depth_' + op.name
    r.u[pd] = r.n[pd] = 1
    r.node = undefined
  }
}

// The term a paren holds when it closes or meets a separator: the child's
// node when that is an expression, or when nothing set the paren's own,
// and otherwise the paren's own (an implicit list, or a value an
// expression or a ternary below wrote here).
function parenTerm(r: Rule): any {
  return isOp(r.child.node) || undefined === r.node ? r.child.node : r.node
}

function makeCloseParen(r: Rule) {
  r.node = parenTerm(r)

  const op = makeOp(r.c0, { [r.c0.tin]: r.u.expr_paren_op })
  let pd = 'expr_paren_depth_' + op.name

  // Construct completed paren expression.
  if (r.u[pd] === r.n[pd]) {

    const val = r.node

    // The content's terms: the one term, or each term of a separated
    // content, the earlier ones collected at their separators.
    const terms: any[] = undefined === r.u.expr_items ? [] : [...r.u.expr_items]
    if (undefined !== val) {
      terms.push(val)
    }

    // A postfix paren extends the node its expr rule placed in the tree,
    // `[op, target]`, in place.
    const postfix = r.parent?.u?.expr_postfix
    if (postfix) {
      postfix[0] = op
      postfix.push(...terms)
      r.node = postfix
      return
    }

    // r.node = [op.osrc]
    r.node = [op]

    // WWW
    if (r.parent.parent?.u?.paren_preval
      && undefined !== r.parent.parent.node) {
      r.node.push(r.parent.parent.node)
    }

    r.node.push(...terms)
  }
}


// Start a top-level implicit list, publishing it to the rule's parent —
// unless that parent is the NORULE sentinel.
//
// NORULE carries a `node` like any other rule, and a rule seeds its node
// from its parent, so a list written there is handed straight back out as
// the starting node of the rules that follow. `@x!, 3` reaches here with
// NORULE as its parent, because the root val closed early to let the
// suffix take the completed prefix: the `elem` that should have collected
// `3` was seeded with the list instead, `3` was dropped, and the list
// ended up containing itself. The rule's own node is what the parse
// returns at this depth, so skipping the write costs nothing.
// The completed expression a top-level implicit list should start with.
//
// The rule that meets the separator is the last one prattify built, which
// for a right-leaning tree is a SUB-expression: `1+2*3` leaves it holding
// `[*,2,3]` while the finished `[+,1,[*,2,3]]` sits on the val this
// expression publishes to, and on the outermost expr in the r:-
// replacement chain. Seeding the list from the rule's own node therefore
// dropped the `1+` — `1+2*3,4` parsed as `[[*,2,3],4]`, a wrong answer
// with nothing to signal it. A flat first member (`1+2,3`) is unaffected,
// because there the rule's node already IS the whole expression, which is
// why this went unnoticed.
//
// Only a top-level bare list reaches here; `[1+2*3,4]`, `f(1+2*3,4)` and
// `{a:1+2*3,b:4}` collect their members by other paths and were correct.
function exprRoot(rule: Rule, ctx: Context) {
  const parent: any = rule.parent

  if (parent && ctx.NORULE !== parent && isOp(parent.node)) {
    return parent.node
  }

  // No usable parent — the NORULE case topList handles below. The
  // outermost expr in the chain holds the root; later entries are the
  // sub-expressions built under it.
  let root: any = undefined
  for (let cur: any = rule; null != cur && ctx.NORULE !== cur; cur = cur.prev) {
    if ('expr' === cur.name && isOp(cur.node)) {
      root = cur.node
    }
  }

  return undefined === root ? rule.node : root
}


function topList(rule: Rule, ctx: Context) {
  const list = [exprRoot(rule, ctx)]

  if (rule.parent && ctx.NORULE !== rule.parent) {
    rule.parent.node = list
    return list
  }

  // No parent to publish to. The list is still built correctly by the
  // `elem` rules that follow, but nothing would ever read it: the result
  // came back as the first member alone, with `, 3` silently gone.
  //
  // Write it back along the r:-replacement chain instead, so whichever
  // rule `ctx.root().node` resolves to is holding this list — the same
  // approach the ternary close hook takes. Members appended later are
  // visible through it, because every writer shares this one array.
  for (let cur: any = rule; null != cur && ctx.NORULE !== cur; cur = cur.prev) {
    cur.node = list
  }

  return list
}


function implicitList(rule: Rule, ctx: Context, a: any) {
  let paren: Rule | null = null
  let parenI = -1

  // Find the paren rule that contains this implicit list.
  // If a map or list rule sits between the expression and the paren,
  // the expression is inside a contained value — not a direct paren
  // child — so don't create an implicit list.
  for (let rI = ctx.rsI - 1; -1 < rI; rI--) {
    if ('paren' === ctx.rs[rI].name) {
      paren = ctx.rs[rI]
      parenI = rI
      break
    }
    if ('map' === ctx.rs[rI].name || 'list' === ctx.rs[rI].name) {
      return a
    }
  }

  if (paren) {
    // The node this expression contributes, captured before the
    // reassignment below replaces it with the list.
    const exprNode = rule.node

    // Create a list value for the paren rule.
    if (null == paren.child.node) {
      paren.child.node = [rule.node]
      a.r = 'elem'
      a.b = 0
    }

    // Convert paren value into a list value.
    else if (isOp(paren.child.node)) {
      paren.child.node = [paren.child.node]
      a.r = 'elem'
      a.b = 0
    }

    rule.node = paren.child.node

    // The rule that sees the comma is not always the paren's own child.
    // A first argument that nests operators — a prefix whose operand is
    // itself a prefix, `f(- -1, 2)` — leaves further rules on the stack
    // between the paren and this rule, all holding the same expression
    // node. Each writes that node back over the paren's when it closes,
    // discarding every element collected after the first: `f(- -1, 2)`
    // arrived as one argument, not two. Point them at the implicit list
    // so the close-back is a no-op instead of a truncation.
    //
    // Only the frames that carry this expression: the `expr` rules
    // themselves, and the `val` rules they were pushed from, which hold
    // either nothing yet or the expression node. A rule a host plugin
    // stacked in between has a node of its own to close with, and
    // overwriting it would destroy a value this plugin knows nothing
    // about.
    for (let rI = parenI + 1; rI < ctx.rsI; rI++) {
      const frame = ctx.rs[rI]

      if ('expr' === frame.name ||
        ('val' === frame.name &&
          (null == frame.node || exprNode === frame.node || isOp(frame.node)))) {
        frame.node = paren.child.node
      }
    }
  }

  return a
}


function implicitTernaryCond(r: Rule) {
  let cond =
    (0 === r.d || 1 <= r.n.expr_paren) && !r.n.pk && 2 === r.u.expr_ternary_step
  return cond
}

function implicitTernaryAction(r: Rule, _ctx: Context, a: AltMatch) {
  r.n.expr_paren = r.prev.u.expr_ternary_paren
  r.node.push(r.child.node)

  if ('elem' === a.r) {
    r.node[0] = dupNode(r.node)
    r.node.length = 1
  }
}

function isParenOp(node: any) {
  return isOpKind('paren', node)
}

function isTernaryOp(node: any) {
  return isOpKind('ternary', node)
}

function isOpKind(kind: string, node: any) {
  return null == node ? false : isOp(node) && true === node[0][kind]
}

function isOp(node: any) {
  return null == node ? false : node[0] && node[0].OP_MARK === OP_MARK
}

function makeOpMap(
  token: (tkn: string | Tin) => Tin | string,
  fixed: (tkn: string) => Tin,
  op: { [name: string]: OpDef },
  anyfix: 'prefix' | 'suffix' | 'infix' | 'ternary',
): OpMap {
  return Object.entries(op)
    .filter(([_, opdef]: [string, OpDef]) => opdef[anyfix])
    .reduce((odm: OpMap, [name, opdef]: [string, OpDef]) => {
      let tkn = ''
      let tin: Tin = -1 as Tin
      let src = ''

      if ('string' === typeof opdef.src) {
        src = opdef.src
      } else {
        src = (opdef.src as string[])[0]
      }

      tin = (fixed(src) || token('#E' + src)) as Tin
      tkn = token(tin) as string

      let op = (odm[tin] = {
        src: src,
        left: opdef.left || Number.MIN_SAFE_INTEGER,
        right: opdef.right || Number.MAX_SAFE_INTEGER,
        name: name + (name.endsWith('-' + anyfix) ? '' : '-' + anyfix),
        infix: 'infix' === anyfix,
        prefix: 'prefix' === anyfix,
        suffix: 'suffix' === anyfix,
        ternary: 'ternary' === anyfix,
        tkn,
        tin,
        terms: 'ternary' === anyfix ? 3 : 'infix' === anyfix ? 2 : 1,
        use: {} as any,
        paren: false,
        osrc: '',
        csrc: '',
        otkn: '',
        ctkn: '',
        otin: -1,
        ctin: -1,
        preval: {
          active: false,
          required: false,
        },
        content: { required: false, separated: false },
        separator: 'infix' === anyfix && !!opdef.separator,
        cast: false,
        key: false,
        dynamic: false,
        token: {} as Token,
        OP_MARK,
      })

      // Handle the second operator if ternary.
      if (op.ternary) {
        let srcs = opdef.src as string[]
        op.src = srcs[0]
        op.use.ternary = { opI: 0 }

        let op2 = { ...op }
        src = (opdef.src as string[])[1]

        tin = (fixed(src) || token('#E' + src)) as Tin
        tkn = token(tin) as string

        op2.src = src
        op2.use = { ternary: { opI: 1 } }
        op2.tkn = tkn
        op2.tin = tin

        odm[tin] = op2
      }

      return odm
    }, {})
}

// The paren operators, in declaration order. Several may share an
// opener; the plugin picks, per opener, the group and the preval one.
function makeParenOps(
  token: (tkn_tin: string | Tin) => Tin | string,
  fixed: (tkn: string) => Tin,
  optop: { [name: string]: OpDef },
): Op[] {
  return entries(optop).reduce((a: Op[], [name, pdef]: [string, any]) => {
    if (pdef.paren) {
      let otin = (fixed(pdef.osrc) || token('#E' + pdef.osrc)) as Tin
      let otkn = token(otin) as string
      let ctin = (fixed(pdef.csrc) || token('#E' + pdef.csrc)) as Tin
      let ctkn = token(ctin) as string

      a.push({
        name: name + '-paren',
        osrc: pdef.osrc,
        csrc: pdef.csrc,
        otkn,
        otin,
        ctkn,
        ctin,
        preval: {
          // True by default if preval specified.
          active:
            null == pdef.preval
              ? false
              : null == pdef.preval.active
                ? true
                : pdef.preval.active,
          // False by default.
          required:
            null == pdef.preval
              ? false
              : null == pdef.preval.required
                ? false
                : pdef.preval.required,
        },
        content: {
          required: !!pdef.content?.required,
          separated: !!pdef.content?.separated,
        },
        separator: false,
        cast: false,
        use: {} as any,
        paren: true,
        src: pdef.osrc,
        // A binding power on a preval paren: compared like a suffix's
        // with the operator whose operand precedes it (see bindsHere).
        // Unset, the paren binds to the value just before it.
        left: pdef.left || Number.MIN_SAFE_INTEGER,
        right: Number.MAX_SAFE_INTEGER,
        infix: false,
        prefix: false,
        suffix: false,
        ternary: false,
        tkn: '',
        tin: -1,
        terms: 1,
        key: false,
        dynamic: false,
        token: {} as Token,
        OP_MARK,
      })
    }
    return a
  }, [])
}


// The cast operators, by the opener of the group each is written with.
function makeCastOps(
  token: (tkn_tin: string | Tin) => Tin | string,
  fixed: (tkn: string) => Tin,
  optop: { [name: string]: OpDef },
): OpMap {
  return entries(optop).reduce((a: OpMap, [name, cdef]: [string, any]) => {
    if (cdef.cast) {
      let otin = (fixed(cdef.osrc) || token('#E' + cdef.osrc)) as Tin
      let ctin = (fixed(cdef.csrc) || token('#E' + cdef.csrc)) as Tin
      a[otin] = {
        name: name + (name.endsWith('-cast') ? '' : '-cast'),
        src: 'string' === typeof cdef.src ? cdef.src : cdef.osrc + cdef.csrc,
        left: Number.MIN_SAFE_INTEGER,
        right: cdef.right || Number.MAX_SAFE_INTEGER,
        use: cdef.use || {},
        // A prefix, whose first term, the group's content, is fixed when
        // it opens: prattify reads it as one.
        prefix: true,
        suffix: false,
        infix: false,
        ternary: false,
        paren: false,
        terms: 2,
        tkn: '',
        tin: -1,
        osrc: cdef.osrc,
        csrc: cdef.csrc,
        otkn: token(otin) as string,
        otin,
        ctkn: token(ctin) as string,
        ctin,
        preval: { active: false, required: false },
        content: { required: false, separated: false },
        separator: false,
        cast: true,
        accept: cdef.accept,
        key: false,
        dynamic: false,
        token: {} as Token,
        OP_MARK,
      }
    }
    return a
  }, {})
}


// The default grouping paren, as `Expr.defaults` defines it, is left out
// of a table that has another paren operator opening with `(` and closing
// with `)`, as the Go and Rust ports leave it out. A `plain` the options
// change is theirs, and is kept.
function dropDefaultPlain(optop: { [name: string]: OpDef }) {
  const plain: any = optop.plain
  const def: any = (Expr.defaults as ExprOptions).op?.plain
  if (null == plain || null == def ||
    Object.keys(plain).length !== Object.keys(def).length ||
    Object.keys(def).some((k) => plain[k] !== def[k])) {
    return
  }
  for (const [name, pdef] of entries(optop) as [string, any][]) {
    if ('plain' !== name && null != pdef && pdef.paren &&
      def.osrc === pdef.osrc && def.csrc === pdef.csrc) {
      delete optop.plain
      return
    }
  }
}


// Key operators, by the member name that marks a site. A key operator
// carries no token, no tin and no binding power; `terms` is 2 (the
// member's value, then the sibling map). The table is a Map in the order
// a map that owns two of the members is resolved by: static operators
// before dynamic ones, then member names in code-point order. Neither
// declaration order nor object key order is used: a Go option table has
// no declaration order, and a JavaScript object lists integer-like names
// first. Two operators on one member name are refused.
function makeKeyMap(optop: { [name: string]: OpDef }): KeyMap {
  const found: Op[] = []
  const owner = new Map<string, string>()
  for (const [name, kdef] of entries(optop) as [string, any][]) {
    if (!kdef || !kdef.key) continue
    const src = 'string' === typeof kdef.src ? kdef.src : (kdef.src || [])[0]
    if ('string' !== typeof src || '' === src) {
      throw new FoldError('fold_key', 'key operator ' + shown(name) + ' names no member')
    }
    const prior = owner.get(src)
    if (undefined !== prior) {
      throw new FoldError('fold_key',
        'key operators ' + shown(prior) + ' and ' + shown(name) + ' both name the member ' + shown(src))
    }
    owner.set(src, name)
    found.push({
      name: name + (name.endsWith('-key') ? '' : '-key'),
      src,
      left: Number.MIN_SAFE_INTEGER,
      right: Number.MAX_SAFE_INTEGER,
      use: kdef.use || {},
      prefix: false,
      suffix: false,
      infix: false,
      ternary: false,
      paren: false,
      terms: 2,
      tkn: '',
      tin: -1,
      osrc: '',
      csrc: '',
      otkn: '',
      otin: -1,
      ctkn: '',
      ctin: -1,
      preval: { active: false, required: false },
      content: { required: false, separated: false },
      separator: false,
      cast: false,
      key: true,
      dynamic: !!kdef.dynamic,
      token: {} as Token,
      OP_MARK,
    })
  }
  found.sort((a, b) => a.dynamic === b.dynamic ? byCodePoint(a.src, b.src) : a.dynamic ? 1 : -1)
  return new Map(found.map((op) => [op.src, op]))
}

// Compare two strings by Unicode code point, which is the order Go and
// Rust compare strings in; JavaScript's own `<` compares UTF-16 code
// units, which differs above U+FFFF.
function byCodePoint(a: string, b: string): number {
  const x = Array.from(a)
  const y = Array.from(b)
  for (let i = 0; i < x.length && i < y.length; i++) {
    const d = (x[i].codePointAt(0) as number) - (y[i].codePointAt(0) as number)
    if (0 !== d) return d
  }
  return x.length - y.length
}


// A plain record: an object whose prototype is `null` (the engine's
// maps) or `Object.prototype` (`JSON.parse`, a literal). Anything else, a
// Map, a Date, a class instance, a boxed value, is a leaf to a fold and
// to `evaluation`: passed through by identity, never rebuilt, since a
// rebuilt copy would carry the prototype without the internal state.
function isRecord(v: any): boolean {
  if (null == v || 'object' !== typeof v || Array.isArray(v)) return false
  const proto = Object.getPrototypeOf(v)
  return null === proto || Object.prototype === proto
}

// The key operator a map is an occurrence of, by shape: the first, in the
// table's order, whose member the map owns. Undefined for anything else.
function keyOpOf(keys: KeyMap, node: any): Op | undefined {
  if (!isRecord(node)) return undefined
  for (const op of keys.values()) {
    if (Object.prototype.hasOwnProperty.call(node, op.src)) return op
  }
  return undefined
}

// RFC 6901: a pointer from path segments.
function pointer(path: Path): string {
  let out = ''
  for (const seg of path) {
    out += '/' + String(seg).replace(/~/g, '~0').replace(/\//g, '~1')
  }
  return out
}

// An RFC 6901 array index: `0`, or digits with no leading zero, given as
// a string, or a non-negative integer; -1 for anything else, `-` and
// out-of-range indices included.
const INDEX = /^(0|[1-9][0-9]*)$/
function arrayIndex(seg: string | number, length: number): number {
  const i = 'number' === typeof seg
    ? (Number.isInteger(seg) ? seg : -1)
    : (INDEX.test(seg) ? Number(seg) : -1)
  return 0 <= i && i < length ? i : -1
}

// One step of a path: an index into a list, or an own member of a plain
// record. NOTHING when the step names nothing.
const NOTHING = Symbol('nothing')
function step(node: any, seg: string | number): any {
  if (Array.isArray(node)) {
    const i = arrayIndex(seg, node.length)
    return i < 0 ? NOTHING : node[i]
  }
  if (isRecord(node) && Object.prototype.hasOwnProperty.call(node, seg)) return node[seg]
  return NOTHING
}

// A new container of the same prototype as `like` (the engine builds
// null-prototype objects), with members DEFINED rather than assigned so a
// member named `__proto__` stays data.
function newRecord(like: any): any {
  return Object.create(Object.getPrototypeOf(like))
}
function define(o: any, k: string, v: any): void {
  Object.defineProperty(o, k, { value: v, enumerable: true, writable: true, configurable: true })
}


// The fold's own errors. `code` is what a fixture pins: `fold_cycle` (a
// cycle under `cycle: 'error'`), `fold_depth` (nesting and reference hops
// past `maxDepth`, or the stack ran out first), `fold_inherit` (an
// `inherit` that returned no bag, or folded its own subtree), `fold_key`
// (an operator table with two key operators on one member name, or one
// on none) and `fold_option` (`positions: true` outside deferred mode).
// The document key and the pointers are fields; the message quotes them
// JSON-escaped and cut short, since member names are document content.
class FoldError extends Error {
  code: string
  doc?: string
  at?: string
  targetDoc?: string
  target?: string
  constructor(
    code: string,
    message: string,
    fields?: { doc?: string; at?: string; targetDoc?: string; target?: string },
  ) {
    super('expr fold: ' + message)
    this.name = 'FoldError'
    this.code = code
    if (fields) Object.assign(this, fields)
  }
}

// A key or pointer for a message: JSON-escaped, so a newline or an escape
// sequence in a member name stays visible, and at most about 80
// characters long.
function shown(s: string): string {
  const q = JSON.stringify(s)
  return q.length <= 80 ? q : q.slice(0, 76) + '..."'
}


// Record the opening token of every container a parse builds, by the
// container's identity, and return the lookup `fold` takes as
// `positions`. It subscribes to the engine's post-process rule event on
// the instance, so it works in any grammar; the table is weak, so a
// parsed tree's entries go with the tree.
function recordPositions(tn: Tabnas): Positions {
  const tokens = new WeakMap<object, Token>()
  tn.sub({
    ruleDone: (r: Rule, _ctx: Context, done: any) => {
      if ('o' !== done.state) return
      const node: any = r.node
      if (null != node && 'object' === typeof node && !tokens.has(node) && r.o0) {
        tokens.set(node, r.o0)
      }
    },
  })
  return (node: object) => tokens.get(node)
}


// The fold's memory: one node per container location the fold has
// reached, in a trie that follows the document, so a location's memo,
// state and inherited bag cost a constant amount however deep it is.
// Pointers are rendered only for report rows and errors.
type FoldDoc = { key: string; root: any; top: PNode }

class PNode {
  kids: Map<string, PNode> | undefined = undefined
  // 0 not yet folded, 1 being folded (on the active stack), 2 folded.
  state = 0
  value: any = undefined
  // How the walk entered it: 0 from its parent, 1 through a reference.
  via = 0
  k: Record<string, any> | undefined = undefined
  kBusy = false
  // Its resolution met a cycle, so the resolver was told `cycle: true`.
  cut = false
  // A member of a loop of references with no container in it, or a
  // reference into one: there is nothing to resolve it to.
  loop = false
  // The key operator this map is an occurrence of, decided once (shape,
  // then the host's `recognise`): undefined until decided, null for none.
  site: Op | null | undefined = undefined
  parts: { op: Op; value: any; siblings: any } | undefined = undefined
  plainDone = false
  plainValue: any = undefined
  ptr: string | undefined = undefined
  constructor(
    public doc: FoldDoc,
    public parent: PNode | undefined,
    public seg: string | number,
    public raw: any,
  ) {}
}


// Fold a finished tree: walk it, reduce every key-operator site
// bottom-up through `opts.evaluate`, and return a NEW tree; the input is
// never changed (though a value a resolver returns, `site.node` among
// them, is placed in the output as it is). Values flow up through
// `evaluate`; context flows down through `inherit`, computed per path
// from each map's own ancestors and kept. One result per location, so
// every site naming one target shares one folded value. Every container
// being folded is on an active stack: a site whose target is on it
// closes a cycle, which the resolver is told about (or which throws).
// Dynamic key operators are reported and folded as plain maps. Token
// operator arrays met in the walk are reduced through the same
// `evaluate` when `tokens` is on.
function fold(root: any, opts: FoldOptions): any {
  return foldTree(root, opts, makeKeyMap(opts.op || {}))
}

function foldTree(root: any, opts: FoldOptions, keys: KeyMap): any {
  if (true === opts.positions) {
    throw new FoldError('fold_option',
      'positions: true records tokens during a parse, so it works in the deferred mode only;'
      + ' pass a lookup, such as recordPositions(tn), to fold')
  }

  const tokens = false !== opts.tokens
  const inherit = opts.inherit
  const recognise = opts.recognise
  const lookup = 'function' === typeof opts.positions ? opts.positions : undefined
  const maxDepth = null == opts.maxDepth ? 1000 : opts.maxDepth
  const base = opts.k || {}
  const report: FoldReport = { cycles: [], unresolved: [], dynamic: [] }
  const noted = new Set<string>()
  const stack: PNode[] = []
  const docs = new Map<string, FoldDoc | null>()

  const makeDoc = (key: string, raw: any): FoldDoc => {
    const doc = { key, root: raw } as FoldDoc
    doc.top = new PNode(doc, undefined, '', raw)
    docs.set(key, doc)
    return doc
  }

  // Another document, by key, loaded once; undefined when it cannot be had.
  const docOf = (from: PNode, key?: string): FoldDoc | undefined => {
    if (undefined === key || key === from.doc.key) return from.doc
    let doc = docs.get(key)
    if (undefined === doc) {
      const raw = undefined === opts.load ? undefined : opts.load(key, siteOf(from))
      doc = undefined === raw ? null : makeDoc(key, raw)
      docs.set(key, doc)
    }
    return null === doc ? undefined : doc
  }

  const kid = (pn: PNode, seg: string | number, raw: any): PNode => {
    const name = String(seg)
    let kids = pn.kids
    if (undefined === kids) kids = pn.kids = new Map()
    let node = kids.get(name)
    if (undefined === node) {
      node = new PNode(pn.doc, pn, seg, raw)
      kids.set(name, node)
    }
    return node
  }

  const ptrOf = (pn: PNode): string => {
    if (undefined !== pn.ptr) return pn.ptr
    const chain: PNode[] = []
    let p: PNode | undefined = pn
    while (undefined !== p && undefined === p.ptr) {
      chain.push(p)
      p = p.parent
    }
    let s = undefined === p ? '' : (p.ptr as string)
    for (let i = chain.length - 1; 0 <= i; i--) {
      const c = chain[i]
      s = undefined === c.parent ? '' : s + '/' + String(c.seg).replace(/~/g, '~0').replace(/\//g, '~1')
      c.ptr = s
    }
    return s
  }

  const pathOf = (pn: PNode): Path => {
    const path: Path = []
    for (let p: PNode | undefined = pn; undefined !== p && undefined !== p.parent; p = p.parent) {
      path.push(p.seg)
    }
    return path.reverse()
  }

  const note = (list: 'cycles' | 'unresolved' | 'dynamic', row: any) => {
    const id = JSON.stringify([list, row.doc, row.at, row.targetDoc, row.target, row.src])
    if (noted.has(id)) return
    noted.add(id)
    report[list].push(row)
  }

  // The bag at a map: the `inherit` calls of its ancestors and of the map
  // itself, from the root down, each kept, worked out without recursion.
  const kOf = (pn: PNode): Record<string, any> => {
    if (undefined === inherit) return base
    if (undefined !== pn.k) return pn.k
    const chain: PNode[] = []
    let p: PNode | undefined = pn
    while (undefined !== p && undefined === p.k) {
      if (p.kBusy) {
        throw new FoldError('fold_inherit', 'inherit folded its own subtree at ' + shown(ptrOf(p)),
          { doc: p.doc.key, at: ptrOf(p) })
      }
      chain.push(p)
      p = p.parent
    }
    let k: Record<string, any> = undefined === p ? base : (p.k as Record<string, any>)
    for (let i = chain.length - 1; 0 <= i; i--) {
      const c = chain[i]
      if (isRecord(c.raw)) {
        c.kBusy = true
        let next: any
        try {
          next = inherit(siteOf(c, k), c.raw, k)
        }
        finally {
          c.kBusy = false
        }
        if (null == next || 'object' !== typeof next) {
          throw new FoldError('fold_inherit', 'inherit returned no bag at ' + shown(ptrOf(c)),
            { doc: c.doc.key, at: ptrOf(c) })
        }
        k = next
      }
      c.k = k
    }
    return k
  }

  const siteOf = (pn: PNode, parentK?: Record<string, any>): Site => {
    let path: Path | undefined
    const node = pn.raw
    return {
      root: pn.doc.root,
      doc: pn.doc.key,
      get path() {
        return undefined === path ? (path = pathOf(pn)) : path
      },
      node,
      get k() {
        return undefined === parentK ? kOf(pn) : parentK
      },
      token: undefined !== lookup && null != node && 'object' === typeof node ? lookup(node) : undefined,
      at: (target: Path, doc?: string) => at(pn, target, doc),
      locate: (target: Path, doc?: string) => locate(pn, target, doc),
      plain: () => plainOf(pn),
    }
  }

  const locate = (from: PNode, target: Path, key?: string): { found: boolean; value?: any } => {
    const doc = docOf(from, key)
    if (undefined === doc) return { found: false }
    let node = doc.root
    for (const seg of target) {
      node = step(node, seg)
      if (NOTHING === node) return { found: false }
    }
    return { found: true, value: node }
  }

  const miss = (from: PNode, targetDoc: string, target: Path): FoldHit => {
    note('unresolved', { doc: from.doc.key, at: ptrOf(from), targetDoc, target: pointer(target) })
    return { found: false, cycle: false }
  }

  // A cycle: `from` named `target` while `target` was still being folded
  // (a back edge), or `target` is in a loop of references. Every member of
  // a loop made only of references is a cycle, and so is every reference
  // into one, since none of them has anything to resolve to. A cycle that
  // passes through a container is cut once, at the site whose target was
  // being folded; a later reference to that site gets its folded value.
  const cycleTo = (from: PNode, target: PNode): FoldHit => {
    const row = { doc: from.doc.key, at: ptrOf(from), targetDoc: target.doc.key, target: ptrOf(target) }
    if ('error' === opts.cycle) {
      throw new FoldError('fold_cycle', 'cycle at ' + shown(row.at) + ' to ' + shown(row.target), row)
    }
    note('cycles', row)
    from.cut = true
    if (1 === target.state) {
      const i = stack.lastIndexOf(target)
      let refs = 0 <= i
      for (let j = i + 1; refs && j < stack.length; j++) refs = 1 === stack[j].via
      if (refs) {
        for (let j = i; j < stack.length; j++) stack[j].loop = true
        from.loop = true
      }
    }
    else if (target.loop) {
      from.loop = true
    }
    return { found: true, cycle: true }
  }

  // The folded value at `target` in a document, for the site at `from`.
  // Raw members are followed from the root; a key-operator map met on the
  // way is folded (through the memo) and the remaining segments read its
  // folded value, so a reference to a reference resolves. A target still
  // being folded is a cycle, and so is a path that must pass through a
  // site that was cut.
  const at = (from: PNode, target: Path, key?: string): FoldHit => {
    const targetDoc = undefined === key ? from.doc.key : key
    const doc = docOf(from, key)
    if (undefined === doc) return miss(from, targetDoc, target)
    let pn = doc.top
    let node: any = doc.root
    for (let i = 0; i < target.length; i++) {
      const v = step(node, target[i])
      if (NOTHING === v) return miss(from, targetDoc, target)
      if (i === target.length - 1 && !Array.isArray(v) && !isRecord(v)) {
        return { found: true, cycle: false, value: v }
      }
      pn = kid(pn, Array.isArray(node) ? arrayIndex(target[i], node.length) : String(target[i]), v)
      node = v
      // Only a static site changes what lies below it; any other map,
      // a dynamic one or one `recognise` refused, folds member by member,
      // so the raw path below it names the same location.
      const op = i < target.length - 1 && isRecord(node) ? siteOpOf(pn) : null
      if (null !== op && !op.dynamic) {
        if (1 === pn.state || pn.loop) return cycleTo(from, pn)
        let folded = walk(pn, 1)
        if (pn.loop || pn.cut) return cycleTo(from, pn)
        for (i++; i < target.length; i++) {
          folded = step(folded, target[i])
          if (NOTHING === folded) return miss(from, targetDoc, target)
        }
        return { found: true, cycle: false, value: folded }
      }
    }
    if (1 === pn.state) return cycleTo(from, pn)
    const value = walk(pn, 1)
    if (pn.loop) return cycleTo(from, pn)
    return { found: true, cycle: false, value }
  }

  // The key operator a map is an occurrence of here, if any: its shape,
  // then the host's `recognise`, decided once per location.
  const siteOpOf = (pn: PNode): Op | null => {
    if (undefined !== pn.site) return pn.site
    let op: Op | null = keyOpOf(keys, pn.raw) ?? null
    pn.site = op
    if (null !== op && undefined !== recognise && !recognise(siteOf(pn), op)) op = pn.site = null
    return op
  }

  // A member's folded value: a container through the memo, anything else
  // as it is.
  const member = (pn: PNode, seg: string | number, child: any): any =>
    Array.isArray(child) || isRecord(child) ? walk(kid(pn, seg, child), 0) : child

  // A map folded as a plain map, members in its own order.
  const plainOf = (pn: PNode): any => {
    if (pn.plainDone) return pn.plainValue
    const node = pn.raw
    if (!isRecord(node)) return walk(pn, 0)
    const parts = pn.parts
    const out = newRecord(node)
    for (const name of Object.keys(node)) {
      define(out, name,
        undefined === parts ? member(pn, name, node[name])
          : name === parts.op.src ? parts.value : parts.siblings[name])
    }
    pn.plainDone = true
    pn.plainValue = out
    return out
  }

  const walk = (pn: PNode, via: number): any => {
    if (2 === pn.state) return pn.value
    const node = pn.raw
    const list = Array.isArray(node)
    if (!list && (!isRecord(node) || OP_MARK === node.OP_MARK)) return node
    if (1 === pn.state) return node
    if (maxDepth <= stack.length) {
      throw new FoldError('fold_depth',
        'nesting and references deeper than ' + maxDepth + ' at ' + shown(ptrOf(pn)),
        { doc: pn.doc.key, at: ptrOf(pn) })
    }
    pn.state = 1
    pn.via = via
    stack.push(pn)
    let out: any
    try {
      if (list && isOp(node)) {
        // Each term is folded at its own path first, so a key-operator
        // site inside a token expression resolves, and a nested
        // expression is kept by path. With `tokens` off the operator
        // node is rebuilt around its own head, so it stays an operator.
        const terms: any[] = []
        for (let i = 1; i < node.length; i++) terms.push(member(pn, i, node[i]))
        out = tokens ? opts.evaluate(siteOf(pn), node[0], terms) : [node[0], ...terms]
      }
      else if (list) {
        // Members are walked inline, not through `member`, so a level of
        // nesting costs one stack frame.
        out = []
        for (let i = 0; i < node.length; i++) {
          const c = node[i]
          out.push(Array.isArray(c) || isRecord(c) ? walk(kid(pn, i, c), 0) : c)
        }
      }
      else {
        const op = siteOpOf(pn)
        if (null === op || op.dynamic) {
          if (null !== op) note('dynamic', { doc: pn.doc.key, at: ptrOf(pn), src: op.src })
          if (pn.plainDone) {
            out = pn.plainValue
          }
          else {
            out = newRecord(node)
            for (const name of Object.keys(node)) {
              const c = node[name]
              define(out, name, Array.isArray(c) || isRecord(c) ? walk(kid(pn, name, c), 0) : c)
            }
            pn.plainDone = true
            pn.plainValue = out
          }
        }
        else {
          const siblings = newRecord(node)
          let value: any
          for (const name of Object.keys(node)) {
            const v = member(pn, name, node[name])
            if (name === op.src) value = v
            else define(siblings, name, v)
          }
          pn.parts = { op, value, siblings }
          const site = siteOf(pn)
          out = opts.evaluate(site, { ...op, token: site.token || op.token }, [value, siblings])
        }
      }
    }
    finally {
      stack.pop()
      pn.state = 0
    }
    pn.state = 2
    pn.value = out
    return out
  }

  const main = makeDoc(null == opts.doc ? '' : opts.doc, root)
  let out: any
  try {
    out = walk(main.top, 0)
  }
  catch (err: any) {
    if (err instanceof RangeError && /call stack/i.test(String(err.message))) {
      throw new FoldError('fold_depth',
        'the stack ran out before maxDepth (' + maxDepth + ') was reached; lower maxDepth',
        { doc: main.key })
    }
    throw err
  }
  if (opts.report) opts.report(report)
  return out
}

// Default operator precedence — the "binding power" scale.
//
// Binding powers are compared only by ORDER, never by magnitude (every
// comparison in prattify() is a `<` / `<=`): a higher number binds tighter.
// Associativity is set by left vs right — left < right is left-associative
// (a-b-c => (a-b)-c); left > right is right-associative. An unset left/right
// falls back to MIN/MAX_SAFE_INTEGER (binds loosest on the left, tightest on
// the right), which is why prefix ops give only `right` and parens give
// neither.
//
// The defaults occupy a compact low block on a base-1000000 ladder, leaving the
// entire range above the built-ins open for client operators. Tiers are 1000000
// apart; the +100000 on `right` is the left-associativity offset, so a tier
// occupies a 100000-wide band inside its 1000000-wide slot and adjacent tiers
// can never overlap. The unary prefix is the tightest built-in (4000000);
// everything above it is free for tighter client operators (exponent, postfix,
// call/member, …), and looser operators (assignment, ternary, logical, …) slot
// in below addition:
//
//    <1000000  looser client ops (assignment, ternary, logical, comparison, …)
//     1000000  sequence / comma
//     2000000  addition / subtraction                 <-- built-in
//     3000000  multiplication / division / remainder  <-- built-in
//     4000000  unary prefix      (+ -)                 <-- built-in (tightest)
//     5000000  exponent          (** ^, right-assoc: left 5100000, right 5000000)
//     6000000  postfix / suffix  (! ? ++)
//     7000000  call / index / member  (f() a[i] a.b)
//     8000000+ free — the whole range above 4000000 is open for client ops
//
// To add an operator, pick a tier base = N*1000000 (left = base, right = base +
// 100000 for left-assoc; swap them for right-assoc). Tighter than the built-ins?
// Use 5000000 and up. Looser? Use below 2000000. Sub-divide any gap with
// base + 200000, base + 400000, … — each 1000000-wide gap holds ~4 sub-tiers.
Expr.defaults = {
  op: {
    positive: {
      prefix: true,
      right: 4000000,
      src: '+',
    },
    negative: {
      prefix: true,
      right: 4000000,
      src: '-',
    },

    // NOTE: all these are left-associative as left < right
    // Example: 2+3+4 === (2+3)+4
    addition: {
      infix: true,
      left: 2000000,
      right: 2100000,
      src: '+',
    },
    subtraction: {
      infix: true,
      left: 2000000,
      right: 2100000,
      src: '-',
    },
    multiplication: {
      infix: true,
      left: 3000000,
      right: 3100000,
      src: '*',
    },
    division: {
      infix: true,
      left: 3000000,
      right: 3100000,
      src: '/',
    },
    remainder: {
      infix: true,
      left: 3000000,
      right: 3100000,
      src: '%',
    },

    plain: {
      paren: true,
      osrc: '(',
      csrc: ')',
    },
  },
} as ExprOptions


// Pratt algorithm embeds next operator.
// NOTE: preserves referential integrity of root expression.
function prattify(expr: any, op?: Op, whence?: string): any[] {
  // console.log('PRATT-START', whence, p(expr), op?.name)

  let out = expr
  let expr_op = expr[0]

  if (op) {
    if (op.infix) {
      // op is lower
      if (expr_op.suffix || op.left <= expr_op.right) {
        updateExprNode(expr, op, dupNode(expr))
      }

      // op is higher
      else {
        const end = expr_op.terms

        if (isOp(expr[end]) && expr[end][0].right < op.left) {
          out = prattify(expr[end], op, 'prattify-infix')
          // console.log('PRATT-INFIX-H0', end, out)
        }
        else {
          out = expr[end] = updateExprNode([], op, expr[end])
          // console.log('PRATT-INFIX-H1', end, out)
        }
      }
    }
    else if (op.prefix) {
      out = expr[expr_op.terms] = updateExprNode([], op)
    }
    else if (op.suffix) {
      if (!expr_op.suffix && expr_op.right <= op.left) {
        const end = expr_op.terms

        // NOTE: special case: higher precedence suffix "drills" into
        // lower precedence prefixes: @@1! => @(@(1!)), not @((@1)!)
        if (
          isOp(expr[end]) &&
          expr[end][0].prefix &&
          expr[end][0].right < op.left
        ) {
          prattify(expr[end], op, 'prattify-suffix')
        }
        else {
          expr[end] = updateExprNode([], op, expr[end])
        }
      }
      else {
        updateExprNode(expr, op, dupNode(expr))
      }
    }
  }

  // console.log('PRATT-END', whence, p(out), op?.name)
  return out
}


// Place a bound preval paren after an expression, as prattify places a
// suffix: drilling into the last term while the operator heading it binds
// no tighter than the paren, and wrapping the rest. Returns the node the
// paren heads, `[op, target]`, which the paren's content then extends.
// NOTE: preserves referential integrity of root expression.
function prattifyPostfix(expr: any, op: Op): any[] {
  const expr_op = expr[0]
  if (!expr_op.suffix && expr_op.right <= op.left) {
    const end = expr_op.terms
    if (isOp(expr[end]) && expr[end][0].prefix && expr[end][0].right < op.left) {
      return prattifyPostfix(expr[end], op)
    }
    return (expr[end] = updateExprNode([], op, expr[end]))
  }
  updateExprNode(expr, op, dupNode(expr))
  return expr
}


// Values `evaluate` has already produced. An implicit list is reduced a
// member at a time as the members close, and again as a whole once the
// list is complete, so without this a member's reduced value would be
// handed back to `evaluate` a second time — and a callback that returns
// another marked S-expression (an identity evaluator, a symbolic
// rewriter) would transform it twice, making the result depend on the
// member's position in the list. Weak, so an entry lasts only as long as
// the value itself.
const EVALUATED = new WeakSet<any>()


function evaluation(
  rule: Rule,
  ctx: Context,
  expr: any,
  evaluate: Evaluate,
  opts?: { objects?: boolean },
) {
  let out = expr ?? null

  if (null == expr || EVALUATED.has(expr)) {
    return out
  }

  if (isOp(expr)) {
    out = evaluate(
      rule,
      ctx,
      expr[0],
      expr.slice(1).map((term: any) => evaluation(rule, ctx, term, evaluate, opts)),
    )
  }

  // An implicit list — `f(1+2, 3+4)`, `1+2, 3+4` — is a plain array, not
  // an op-array, so the branch above does not reach its members. Reduce
  // them, into a NEW array: this is a documented public entry point
  // (`ts/doc/reference.md`) and the parse-once/evaluate-many workflow in
  // `ts/doc/guide.md` needs a caller's tree to survive a reduction
  // intact. Where the list under construction must be updated in place,
  // the expr rule's close hook does that itself — mutating its own tree
  // is the parser's business, not this function's.
  else if (Array.isArray(expr)) {
    out = expr.map((term: any) => evaluation(rule, ctx, term, evaluate, opts))
  }

  // A plain object is passed through unchanged by default, as the
  // reference documents; with `objects` on it is reduced member by member
  // into a NEW object of the same prototype, which is what the eager
  // `evaluate` option already does for an expression inside a map.
  else if (opts && opts.objects && isRecord(expr)) {
    const o = newRecord(expr)
    for (const k of Object.keys(expr)) define(o, k, evaluation(rule, ctx, expr[k], evaluate, opts))
    out = o
  }

  // Only a value this fold PRODUCED is marked: a node passed through
  // unchanged (a plain object, by default) is not, so a later fold with
  // `objects` on still reaches the operators inside it.
  if (null != out && 'object' === typeof out && out !== expr) {
    EVALUATED.add(out)
  }

  // console.log('EXPR-EVAL', expr, '->', out)

  return out
}


function p(node: any, seen?: WeakSet<any>): any {
  seen = seen ?? new WeakSet<any>()
  if (seen.has(node)) {
    return '[CIRCLE]'
  }
  else if (null != node && 'object' === typeof node) {
    seen.add(node)
  }
  if (Array.isArray(node) && 'string' === typeof node[0]?.src) {
    return ['OP' + node[0]?.src, ...(node.slice(1).map((n: any) => p(n, seen)))]
  }
  return node
}


// Reduce a parsed value to the plain S-expression form the shared
// fixtures compare and the translation parts are written against:
// `1+2*3` reads as `['+', 1, ['*', 2, 3]]`. An operation is an array whose
// head is the operator description. The head becomes the operator's
// source text (`src`, which for a paren operator is its opening source),
// and a term never filled, `undefined`, is dropped. Every other array and
// object, an empty one included, is reduced member by member into a new
// value, so the result shares no container with the parse, and anything
// else is returned as it is.
//
// A node that contains itself, which a rewrite can leave behind, reads as
// '[CIRCLE]' rather than recursing forever. Only a node inside itself
// does: a node reached twice by two paths is reduced both times. The test
// helper this replaces marked any node it had seen before, and the two
// agree on every row of the shared fixtures, where no parse shares a
// node; this reading is the Rust port's.
//
// The Rust port's `simplify`, and the Go port's `Simplify` and its
// order-keeping `SimplifyOrdered`, give the same value, and the shared
// fixtures hold them to it.
function simplify(value: any): any {
  const open = new Set<any>()
  const reduce = (node: any): any => {
    if (null == node || 'object' !== typeof node) return node
    if (open.has(node)) return '[CIRCLE]'
    open.add(node)
    const out = Array.isArray(node)
      ? (0 === node.length ? [] : [
        null != node[0] && node[0].src ? node[0].src : reduce(node[0]),
        ...node.slice(1).map(reduce),
      ].filter((term: any) => undefined !== term))
      : omap(node, ([name, member]: [any, any]) => [name, reduce(member)])
    open.delete(node)
    return out
  }
  return reduce(value)
}


// Parse `src` on an instance with the plugin installed, and `simplify`
// the result. The Rust port's `parse_simplified`.
function parseSimplified(tn: Tabnas, src: string, meta?: any): any {
  return simplify(tn.parse(src, meta))
}


// Mark a hand-built operator description as this plugin's, so `isOp` and
// `evaluation` recognise a node headed by it. A sibling plugin that builds
// operator nodes needs this; `simplify` needs it not, reading any
// `src`-headed array. Fills nothing else: pass a complete description.
function opify<T extends object>(op: T): T {
  ;(op as any).OP_MARK = OP_MARK
  return op
}

const testing = {
  prattify,
  opify,
}

// VERSION is this package's version. It MUST equal package.json "version":
// the release orchestrator rewrites both, and the version test fails the
// build if they drift. Mirrors `const VERSION` in go/expr.go.
const VERSION = '0.6.0'

export {
  VERSION,
  Expr,
  evaluation,
  simplify,
  parseSimplified,
  fold,
  recordPositions,
  FoldError,
  opify,
  isOp,
  keyOpOf,
  makeKeyMap,
  testing,
}

export type {
  ExprOptions,
  OpDef,
  Op,
  Evaluate,
  FoldOptions,
  FoldEvaluate,
  FoldHit,
  FoldEntry,
  Inherit,
  Recognise,
  Positions,
  Site,
  FoldReport,
  Path,
  KeyMap,
}

// The translation parts a host composes a translation from (admin ADR-27):
// the manifest and the alchemy sources, generated by ../embed-translate.js.
export { translate } from './translate'
export type { TranslationPart, TranslationParts } from './translate'
