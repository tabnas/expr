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
  // still reduce.
  dynamic?: boolean
}

// Options for the plugin.
type ExprOptions = {
  op?: { [name: string]: OpDef }

  // TODO: define Evalute type
  evaluate?: typeof evaluation

  // Fold the finished document before `parse` returns (deferred mode):
  // the plugin subscribes to the engine's post-process rule event and
  // replaces the root node with `fold(root, options.fold)` on the last
  // pass of the parse. Key operators resolve against the finished root,
  // where `evaluate` above reduces a token expression at its own close.
  fold?: FoldOptions
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
  key: boolean
  dynamic: boolean
  token: Token
  OP_MARK: typeof OP_MARK
}

// Lookup operators by token.
type OpMap = { [tin: number]: Op }

// Lookup key operators by the member name that marks a site, in the
// order the `op` table declared them (the first declared wins when a map
// owns two).
type KeyMap = { [src: string]: Op }

// Resolve the value of an operartion
type Evaluate = (rule: Rule, ctx: Context, op: Op, ...terms: any) => any

// A location in a folded tree, as segments from the root: member names
// and array indices.
type Path = (string | number)[]

// What `fold` hands the resolver for one site: the verbatim root, the
// path to the site, the site's verbatim node, the bag inherited from the
// site's ancestors (`inherit`), the site's opening token when the parse
// recorded one, and the two ways to reach the rest of the tree: `at`
// folds another location (through any key-operator sites on the way,
// memoised, cycle-aware) and `locate` reads it raw. `fold` runs the same
// fold over another root, for a document the resolver loaded itself.
// `rule` and `ctx` are null: a fold runs after the parse.
type Site = {
  root: any
  path: Path
  node: any
  k: Record<string, any>
  token?: Token
  rule: null
  ctx: null
  at: (path: Path) => { found: boolean; cycle: boolean; value?: any }
  locate: (path: Path) => { found: boolean; value?: any }
  fold: (root: any, k?: Record<string, any>) => any
}

// Reduce one site: a key operator with `[value, siblings]`, both already
// folded, or, when `tokens` is on, a token operator met in the walk with
// its folded terms.
type FoldEvaluate = (site: Site, op: Op, terms: any[]) => any

// Extend the bag a node's subtree inherits. Called on the way down for
// every object on a path, from the ancestors' bags, so a site reached
// early through a reference gets ITS ancestors' context, never the
// referrer's: the engine's `k` discipline, for a finished tree.
type Inherit = (site: Site, node: any, k: Record<string, any>) => Record<string, any>

type FoldReport = {
  cycles: { at: string; target: string }[]
  unresolved: { at: string; target: string }[]
  dynamic: { at: string; src: string }[]
}

type FoldOptions = {
  // The operator table; only the `key` entries matter to a fold. The
  // plugin supplies its own table in deferred mode.
  op?: { [name: string]: OpDef }
  evaluate: FoldEvaluate
  inherit?: Inherit
  k?: Record<string, any>
  // A site whose target is still being folded (the target itself, an
  // ancestor of the site, or the root): keep the site verbatim and record
  // the cycle (default), or throw.
  cycle?: 'verbatim' | 'error'
  // Reduce token-operator arrays met in the walk through `evaluate` too,
  // so one fold handles `+` and `$ref`. Default true.
  tokens?: boolean
  // Deferred mode only: record every container's opening token while
  // parsing, grammar-neutrally, so `site.token` carries a position.
  positions?: boolean
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

  const parenOTM: OpMap = makeParenMap(token, fixed, optop)
  const parenCTM: OpMap = omap(parenOTM, ([_, pdef]: [Tin, Op]) => [
    undefined,
    undefined,
    pdef.ctin,
    pdef,
  ])

  let parenFixed = Object.values({ ...parenOTM, ...parenCTM }).reduce(
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
  // pushes nor replaces, outside any probe and not synthesised by
  // recovery. The start rule's own after-close is not that event: a
  // grammar may replace its start rule (yaml's `stream` does on a
  // directive or a document marker), and `parse` returns the original
  // root rule's node, which is what is written here.
  if (options.fold) {
    const foldOptions = options.fold
    const tokensByNode = new WeakMap<object, Token>()
    tn.sub({
      ruleDone: (r: Rule, ctx: Context, done: any) => {
        if (foldOptions.positions && 'o' === done.state) {
          const n: any = r.node
          if (null != n && 'object' === typeof n && !tokensByNode.has(n) && r.o0) {
            tokensByNode.set(n, r.o0)
          }
        }
        if (0 !== r.d || 'c' !== done.state || !done.alt || done.alt.p || done.alt.r || done.probe || done.forced) {
          return
        }
        const root = ctx.root()
        root.node = foldTree(root.node, { ...foldOptions, op: optop }, keyTM, tokensByNode)
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

  const OP = values(parenOTM).map((pdef: any) => pdef.otin)
  const CP = values(parenCTM).map((pdef: any) => pdef.ctin)

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
            const pdef = parenOTM[r.o1.tin]
            const preval = r.o0.resolveVal(r, ctx)
            return pdef.preval.active &&
              (null == pdef.preval.allow || pdef.preval.allow.includes(preval))
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

          c: (r: Rule) => {
            const pdef = parenOTM[r.o0.tin]
            return !pdef.preval.required
          },

          g: 'expr,expr-paren',
        }
        : NONE,
    ]).close([
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
            const pdef = parenOTM[r.c0.tin]
            return pdef.preval.active &&
              undefined !== r.node &&
              (null == pdef.preval.allow || pdef.preval.allow.includes(r.node))
          },
          p: 'expr',
          u: { paren_preval: true },
          g: 'expr,expr-paren,expr-paren-preval-chain',
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
      }
    }).close([
      hasParen && {
        s: [CP],

        // If end of normal list, consume `]` - it's not a close paren.
        b: (r: Rule) => (CS === r.c0.tin && !r.n.expr_paren ? 0 : 1),
      },
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
            r.node = isOp(r.parent.node)
              && !isTernaryOp(r.parent.node)
              && !isParenOp(r.parent.node)
              ? prattify(r.parent.node, op, 'expr-prefix')
              : prior(r, r.parent, op, 'expr-prefix')
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
            r.node = isOp(prev.node)
              ? prattify(prev.node, op, 'expr-suffix')
              : prior(r, prev, op, 'expr-suffix')
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
        {
          c: (r: Rule) => 'paren' === r.child.name,
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

        // TTT
        // hasSuffix
        //   ? {
        //     s: [SUFFIX],
        //     c: (r: Rule) => !!r.n.expr_prefix,
        //     b: 1,
        //     r: 'expr',
        //     g: 'expr,expr-suffix',
        //   }
        //   : NONE,

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
          c: (r: Rule) => r.d <= 0,
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
            !SUFFIX.includes(ctx.t0.tin),
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
        hasParen
          ? {
            s: [OP, CP],
            b: 1,
            g: 'expr,expr-paren,empty',
            c: (r: Rule) =>
              parenOTM[r.o0.tin].name === parenCTM[r.o1.tin].name,
            a: makeOpenParen(parenOTM),
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
            a: makeOpenParen(parenOTM),
          }
          : NONE,
      ])

      .close([
        hasParen
          ? {
            s: [CP],
            c: (r: Rule) => {
              const pdef = parenCTM[r.c0.tin]
              let pd = 'expr_paren_depth_' + pdef.name
              return !!r.n[pd]
            },

            a: makeCloseParen(parenCTM),
            g: 'expr,expr-paren,close',
          }
          : NONE,
      ])

      .ac((r: Rule, _ctx: Context) => {
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
          c: implicitTernaryCond,
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
          c: implicitTernaryCond,
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

function makeOpenParen(parenOTM: OpMap) {
  return function openParen(r: Rule) {
    const op = makeOp(r.o0, parenOTM)
    let pd = 'expr_paren_depth_' + op.name
    r.u[pd] = r.n[pd] = 1
    r.node = undefined
  }
}

function makeCloseParen(parenCTM: OpMap) {
  return function closeParen(r: Rule) {
    if (isOp(r.child.node)) {
      r.node = r.child.node
    } else if (undefined === r.node) {
      r.node = r.child.node
    }

    const op = makeOp(r.c0, parenCTM)
    let pd = 'expr_paren_depth_' + op.name

    // Construct completed paren expression.
    if (r.u[pd] === r.n[pd]) {

      const val = r.node

      // r.node = [op.osrc]
      r.node = [op]

      // WWW
      if (r.parent.parent?.u?.paren_preval
        && undefined !== r.parent.parent.node) {
        r.node.push(r.parent.parent.node)
      }

      if (undefined !== val) {
        r.node.push(val)
      }

      // console.log('MCP', r.i, r.name, p(r.node),
      //   'PP', r.parent.parent?.u, p(r.parent.parent?.node), 'N', p(r.node))

      // WWW
      // if (r.parent.prev.u.paren_preval) {
      //   if (isParenOp(r.parent.prev.node)) {
      //     r.node = updateExprNode(
      //       r.parent.prev.node,
      //       r.node[0],
      //       dupNode(r.parent.prev.node),
      //       r.node[1],
      //     )
      //   } else {
      //     r.node.splice(1, 0, r.parent.prev.node)
      //     r.parent.prev.node = r.node
      //   }
      // }
    }
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

function makeParenMap(
  token: (tkn_tin: string | Tin) => Tin | string,
  fixed: (tkn: string) => Tin,
  optop: { [name: string]: OpDef },
): OpMap {
  return entries(optop).reduce((a: OpMap, [name, pdef]: [string, any]) => {
    if (pdef.paren) {
      let otin = (fixed(pdef.osrc) || token('#E' + pdef.osrc)) as Tin
      let otkn = token(otin) as string
      let ctin = (fixed(pdef.csrc) || token('#E' + pdef.csrc)) as Tin
      let ctkn = token(ctin) as string

      a[otin] = {
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
        use: {} as any,
        paren: true,
        src: pdef.osrc,
        // left: -1,
        // right: -1,
        left: Number.MIN_SAFE_INTEGER,
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
      }
    }
    return a
  }, {}) as OpMap
}


// Key operators, by the member name that marks a site. A key operator
// carries no token, no tin and no binding power; `terms` is 2 (the
// member's value, then the sibling map).
function makeKeyMap(optop: { [name: string]: OpDef }): KeyMap {
  return entries(optop).reduce((a: KeyMap, [name, kdef]: [string, any]) => {
    if (kdef && kdef.key) {
      const src = 'string' === typeof kdef.src ? kdef.src : (kdef.src || [])[0]
      a[src] = {
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
        key: true,
        dynamic: !!kdef.dynamic,
        token: {} as Token,
        OP_MARK,
      }
    }
    return a
  }, {})
}


// A plain object: not an array, not a boxed string, not null.
function isRecord(v: any): boolean {
  return null != v && 'object' === typeof v && !Array.isArray(v) && !(v instanceof String)
}

// The key operator a map is an occurrence of, by shape: the first
// declared key the map owns. Undefined for anything else.
function keyOpOf(keys: KeyMap, node: any): Op | undefined {
  if (!isRecord(node)) return undefined
  for (const src in keys) {
    if (Object.prototype.hasOwnProperty.call(node, src)) return keys[src]
  }
  return undefined
}

// RFC 6901: the memo and cycle key of a path.
function pointer(path: Path): string {
  let out = ''
  for (const seg of path) {
    out += '/' + String(seg).replace(/~/g, '~0').replace(/\//g, '~1')
  }
  return out
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


// Fold a finished tree: walk it, reduce every key-operator site
// bottom-up through `opts.evaluate`, and return a NEW tree; the input is
// never changed. Values flow up through `evaluate`; context flows down
// through `inherit`, computed per path from each node's own ancestors
// and memoised per prefix. One result per path (memo), so every site
// naming one target shares one folded value; every container being
// folded is on an active set, so a site whose target is still being
// built (the target itself, an ancestor, the root) is a cycle: kept
// verbatim and reported, or an error. Dynamic key operators are reported
// and folded as plain maps. Token-operator arrays met in the walk are
// reduced through the same `evaluate` when `tokens` is on.
function fold(root: any, opts: FoldOptions): any {
  return foldTree(root, opts, makeKeyMap(opts.op || {}), undefined)
}

function foldTree(
  root: any,
  opts: FoldOptions,
  keys: KeyMap,
  tokensByNode: WeakMap<object, Token> | undefined,
): any {
  const report: FoldReport = { cycles: [], unresolved: [], dynamic: [] }
  const out = foldRoot(root, opts, keys, tokensByNode, report)
  if (opts.report) opts.report(report)
  return out
}

function foldRoot(
  root: any,
  opts: FoldOptions,
  keys: KeyMap,
  tokensByNode: WeakMap<object, Token> | undefined,
  report: FoldReport,
): any {
  const tokens = false !== opts.tokens
  const memo = new Map<string, any>()
  const active = new Set<string>()
  // Sites whose own resolution met a cycle: a reference INTO a cycle is
  // reported as one too, so every member of a ring is kept verbatim
  // rather than the last one alone.
  const cyclic = new Set<string>()
  const kMemo = new Map<string, Record<string, any>>()
  const inherit: Inherit = opts.inherit || ((_site, _node, k) => k)

  const rawAt = (path: Path): { found: boolean; value?: any } => {
    let node: any = root
    for (const seg of path) {
      if (Array.isArray(node)) node = node[Number(seg)]
      else if (isRecord(node) && Object.prototype.hasOwnProperty.call(node, seg)) node = node[seg]
      else return { found: false }
      if (undefined === node) return { found: false }
    }
    return { found: true, value: node }
  }

  // The bag at a path: the ancestors' `inherit` calls, applied in order
  // from the root, memoised per prefix. Arrays and scalars pass the bag
  // through unchanged.
  const kAt = (path: Path): Record<string, any> => {
    const key = pointer(path)
    const hit = kMemo.get(key)
    if (undefined !== hit) return hit
    let k: Record<string, any> = path.length ? kAt(path.slice(0, -1)) : (opts.k || {})
    const node = rawAt(path).value
    if (isRecord(node)) k = inherit(siteFor(path, node, k), node, k)
    kMemo.set(key, k)
    return k
  }

  const siteFor = (path: Path, node: any, k?: Record<string, any>): Site => ({
    root,
    path,
    node,
    k: k || kAt(path),
    token: null != node && 'object' === typeof node && tokensByNode ? tokensByNode.get(node) : undefined,
    rule: null,
    ctx: null,
    at: (target: Path) => at(path, target),
    locate: rawAt,
    fold: (other: any, k2?: Record<string, any>) =>
      foldRoot(other, { ...opts, k: k2 || opts.k }, keys, tokensByNode, report),
  })

  // The folded value at `target`, for the resolver at `from`. Raw
  // segments are followed from the root; a static key-operator site met
  // on the way is folded (through the memo) and the remaining segments
  // continue inside its folded value, so a reference to a reference
  // resolves. A target still being folded is a cycle.
  const at = (from: Path, target: Path): { found: boolean; cycle: boolean; value?: any } => {
    let node: any = root
    const cur: Path = []
    for (let i = 0; i < target.length; i++) {
      const seg = target[i]
      cur.push(seg)
      if (Array.isArray(node)) node = node[Number(seg)]
      else if (isRecord(node) && Object.prototype.hasOwnProperty.call(node, seg)) node = node[seg]
      else node = undefined
      if (undefined === node) {
        report.unresolved.push({ at: pointer(from), target: pointer(target) })
        return { found: false, cycle: false }
      }
      const op = keyOpOf(keys, node)
      if (op && !op.dynamic && i < target.length - 1) {
        if (active.has(pointer(cur)) || cyclic.has(pointer(cur))) {
          report.cycles.push({ at: pointer(from), target: pointer(cur) })
          cyclic.add(pointer(from))
          return { found: true, cycle: true }
        }
        let v: any = walk(node, cur.slice())
        for (i++; i < target.length; i++) {
          const s = target[i]
          if (Array.isArray(v)) v = v[Number(s)]
          else if (isRecord(v) && Object.prototype.hasOwnProperty.call(v, s)) v = v[s]
          else v = undefined
          if (undefined === v) {
            report.unresolved.push({ at: pointer(from), target: pointer(target) })
            return { found: false, cycle: false }
          }
        }
        return { found: true, cycle: false, value: v }
      }
    }
    if (active.has(pointer(target))) {
      report.cycles.push({ at: pointer(from), target: pointer(target) })
      cyclic.add(pointer(from))
      return { found: true, cycle: true }
    }
    const value = walk(node, target)
    if (cyclic.has(pointer(target))) {
      report.cycles.push({ at: pointer(from), target: pointer(target) })
      cyclic.add(pointer(from))
      return { found: true, cycle: true, value }
    }
    return { found: true, cycle: false, value }
  }

  const walk = (node: any, path: Path): any => {
    const key = pointer(path)
    if (memo.has(key)) return memo.get(key)
    if (active.has(key)) {
      if ('error' === opts.cycle) throw new Error('expr fold: cycle at ' + key)
      return node
    }
    active.add(key)
    let out: any
    try {
      if (tokens && isOp(node)) {
        out = evaluation(null as any, null as any, node, (_r: any, _c: any, op: Op, terms: any[]) =>
          opts.evaluate(siteFor(path, node), op, terms))
      }
      else if (Array.isArray(node)) {
        out = []
        for (let i = 0; i < node.length; i++) out.push(walk(node[i], [...path, i]))
      }
      else if (isRecord(node)) {
        const op = keyOpOf(keys, node)
        if (op && !op.dynamic) {
          const siblings = newRecord(node)
          for (const k of Object.keys(node)) {
            if (k !== op.src) define(siblings, k, walk(node[k], [...path, k]))
          }
          const value = walk(node[op.src], [...path, op.src])
          const site = siteFor(path, node)
          const cycles = report.cycles.length
          out = opts.evaluate(site, { ...op, token: site.token || op.token }, [value, siblings])
          if ('error' === opts.cycle && report.cycles.length > cycles) {
            throw new Error('expr fold: cycle at ' + key)
          }
        }
        else {
          if (op) report.dynamic.push({ at: key, src: op.src })
          out = newRecord(node)
          for (const k of Object.keys(node)) define(out, k, walk(node[k], [...path, k]))
        }
      }
      else {
        out = node
      }
    }
    finally {
      active.delete(key)
    }
    memo.set(key, out)
    return out
  }

  return walk(root, [])
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
const VERSION = '0.5.19'

export { VERSION, Expr, evaluation, simplify, parseSimplified, fold, opify, isOp, keyOpOf, makeKeyMap, testing }

export type { ExprOptions, OpDef, Op, Evaluate, FoldOptions, FoldEvaluate, Inherit, Site, FoldReport, Path, KeyMap }

// The translation parts a host composes a translation from (admin ADR-27):
// the manifest and the alchemy sources, generated by ../embed-translate.js.
export { translate } from './translate'
export type { TranslationPart, TranslationParts } from './translate'
