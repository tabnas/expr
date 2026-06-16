// Generate TSV spec files from parser output
const { Jsonic, util } = require('jsonic')
const { Expr } = require('..')
const fs = require('fs')
const path = require('path')

const { omap } = util

const C = (x) => JSON.parse(JSON.stringify(x))
const S = (x, seen) => (
  seen = seen ?? new WeakSet(),
  seen?.has(x) ? '[CIRCLE]' : (
    (x && 'object' === typeof x ? seen?.add(x) : null),
    (x && Array.isArray(x)) ?
      (0 === x.length ? x : [
        x[0].src || S(x[0], seen),
        ...(1 < x.length ? (x.slice(1).map((t) => S(t, seen))) : [])]
        .filter(t => undefined !== t)) :
      (null != x && 'object' === typeof (x) ? omap(x, ([n, v]) => [n, S(v, seen)]) : x)))

const mj = (je) => (s, m) => C(S(je(s, m)))

function writeTsv(filename, header, entries) {
  const lines = [`# ${header}`, '# input\texpected_output']
  for (const [input, output] of entries) {
    lines.push(`${input}\t${JSON.stringify(output)}`)
  }
  const specDir = path.join(__dirname, '..', '..', 'test', 'spec')
  fs.mkdirSync(specDir, { recursive: true })
  fs.writeFileSync(path.join(specDir, filename), lines.join('\n') + '\n')
  console.log(`Wrote ${filename}: ${entries.length} entries`)
}


// === happy ===
{
  const j = mj(Jsonic.make().use(Expr))
  writeTsv('happy.tsv', 'Happy path basic tests - default Expr config', [
    ['1+2', j('1+2')],
    ['-1+2', j('-1+2')],
  ])
}

// === binary ===
{
  const j = mj(Jsonic.make().use(Expr))
  const entries = []
  const cases = [
    '1+2', '1*2',
    '1*2+3', '1+2*3', '1*2*3',
    '1+2+3+4',
    '1*2+3+4', '1+2*3+4', '1+2+3*4',
    '1+2*3*4', '1*2+3*4', '1*2*3+4',
    '1*2*3*4',
    '1+2+3+4+5',
    '1*2+3+4+5', '1+2*3+4+5', '1+2+3*4+5', '1+2+3+4*5',
    '1*2*3+4+5', '1+2*3*4+5', '1+2+3*4*5',
    '1*2+3+4*5', '1*2+3*4+5', '1+2*3+4*5',
    '1+2*3*4*5', '1*2+3*4*5', '1*2*3+4*5', '1*2*3*4+5',
    '1*2*3*4*5',
  ]
  for (const c of cases) {
    entries.push([c, j(c)])
  }
  writeTsv('binary.tsv', 'Binary infix operator tests - default Expr config', entries)
}

// === structure ===
{
  const j = mj(Jsonic.make().use(Expr))
  writeTsv('structure.tsv', 'Expression structure tests - default Expr config', [
    ['a:1+2', j('a:1+2')],
    ['a:1+2,b:3+4', j('a:1+2,b:3+4')],
    ['[1+2]', j('[1+2]')],
    ['[1+2,3+4]', j('[1+2,3+4]')],
    ['{a:[1+2]}', j('{a:[1+2]}')],
  ])
}

// === unary-prefix-basic ===
{
  const j = mj(Jsonic.make().use(Expr))
  const entries = []
  const cases = [
    '-1', '- 1', '+1', '+ 1',
    '--1', '---1', '++1', '+++1',
    '-+1', '+-1',
    '--+1', '-+-1', '+--1',
    '-++1', '++-1',
    '-z', '- z', '+z', '+ z',
    '--z', '---z', '++z', '+++z',
    '-+z', '+-z',
    '--+z', '-+-z', '+--z',
    '-++z', '++-z',
    '1+2', '-1+2', '--1+2',
    '-1+-2', '1+-2', '1++2', '-1++2',
    '-1+2+3', '-1+-2+3', '-1+-2+-3', '-1+2+-3',
    '1+2+3', '1+-2+3', '1+-2+-3', '1+2+-3',
  ]
  for (const c of cases) {
    entries.push([c, j(c)])
  }
  writeTsv('unary-prefix-basic.tsv', 'Unary prefix operator tests - default Expr config', entries)
}

// === paren-basic ===
{
  const j = mj(Jsonic.make().use(Expr))
  const entries = []
  const cases = [
    '100+200', '(100)', '(100)+200', '100+(200)',
    '(1+2)', '(1+2+3)', '(1+2+3+4)',
    '((1))', '(((1)))', '((((1))))',
    '(1+2)+3', '1+(2+3)',
    '((1+2))+3', '1+((2+3))',
    '(1)+2+3',
    '100+200+300', '100+(200)+300',
    '1+2+(3)', '1+(2)+(3)', '(1)+2+(3)', '(1)+(2)+3', '(1)+(2)+(3)',
    '(1+2)*3', '1*(2+3)',
    '(a)', '("a")', '([])', '([a])', '([a,b])', '([a b])',
    '([a,b,c])', '([a b c])',
    '({})', '({a:1})', '({a:1,b:2})', '({a:1 b:2})',
    '({a:1,b:2,c:3})', '({a:1 b:2 c:3})',
    '(a:1)',
    '()', '(),()', '(),(),()',
    '() ()', '() () ()',
    '[()]', '[(),()]', '[(),(),()]',
    '[() ()]', '[() () ()]',
    '{a:()}', '{a:(),b:()}', '{a:(),b:(),c:()}',
    '{a:() b:()}', '{a:() b:() c:()}',
  ]
  for (const c of cases) {
    entries.push([c, j(c)])
  }
  writeTsv('paren-basic.tsv', 'Parenthesis tests - default Expr config', entries)
}

// === implicit-list-top-basic ===
{
  const j = mj(Jsonic.make().use(Expr))
  const entries = []
  const cases = [
    '1,2', '1+2,3', '1+2+3,4', '1+2+3+4,5',
    '1 2', '1+2 3', '1+2+3 4', '1+2+3+4 5',
    '1,2,11', '1+2,3,11', '1+2+3,4,11', '1+2+3+4,5,11',
    '1 2 11', '1+2 3 11', '1+2+3 4 11', '1+2+3+4 5 11',
    '22,1,2,11', '22,1+2,3,11', '22,1+2+3,4,11',
    '22,1+2+3+4,5,11',
    '22 1 2 11', '22 1+2 3 11', '22 1+2+3 4 11',
    '22 1+2+3+4 5 11',
  ]
  for (const c of cases) {
    entries.push([c, j(c)])
  }
  writeTsv('implicit-list-top-basic.tsv', 'Implicit list at top level - default Expr config', entries)
}

// === unary-suffix-basic === (requires custom config)
{
  const je = Jsonic.make().use(Expr, {
    op: {
      factorial: { suffix: true, left: 18000, src: '!' },
      question: { suffix: true, left: 14000, src: '?' },
    }
  })
  const j = mj(je)
  const entries = []
  const cases = [
    '1!', '1 !', '1!!', '1!!!',
    'z!', 'z !',
    '1?', '1 ?', '1??', '1???',
    '1+2!', '1!+2', '1!+2!',
    '1+2!!', '1!!+2', '1!!+2!!',
    '1+2?', '1?+2', '1?+2?',
    '1+2??', '1??+2', '1??+2??',
    '0+1+2!', '0+1!+2', '0+1!+2!',
    '0!+1!+2!', '0!+1!+2', '0!+1+2!', '0!+1+2',
  ]
  for (const c of cases) {
    entries.push([c, j(c)])
  }
  writeTsv('unary-suffix-basic.tsv', 'Unary suffix operator tests - config:suffix', entries)
}

// === unary-suffix-edge === (requires custom config)
{
  const je = Jsonic.make().use(Expr, {
    op: {
      factorial: { suffix: true, left: 18000, src: '!' },
      question: { suffix: true, left: 14000, src: '?' },
      tight: { infix: true, left: 19000, right: 19010, src: '~' },
    }
  })
  const j = mj(je)
  const entries = []
  const cases = [
    '1!', '1!!', '1!!!',
    '1!?', '1?!', '1!??', '1??!',
    '1?!!', '1!!?', '1?!?', '1!?!',
    '1!+2', '1+2!', '1!+2!',
    '1!+2+3', '1+2!+3', '1!+2!+3',
    '1!+2+3!', '1+2!+3!', '1!+2!+3!',
    '1!~2', '1~2!', '1!~2!',
    '1!~2+3', '1~2!+3', '1!~2!+3',
    '1!~2~3', '1~2!~3', '1!~2!~3',
  ]
  for (const c of cases) {
    entries.push([c, j(c)])
  }
  writeTsv('unary-suffix-edge.tsv', 'Unary suffix edge cases - config:suffix-tight', entries)
}

// === unary-suffix-structure ===
{
  const je = Jsonic.make().use(Expr, {
    op: {
      factorial: { suffix: true, left: 18000, src: '!' },
      question: { suffix: true, left: 14000, src: '?' },
    }
  })
  const j = mj(je)
  const entries = []
  const cases = [
    '1!,2!', '1!,2!,3!', '1!,2!,3!,4!',
    '1! 2!', '1! 2! 3!', '1! 2! 3! 4!',
    '[1!,2!]', '[1!,2!,3!]', '[1!,2!,3!,4!]',
    '[1! 2!]', '[1! 2! 3!]', '[1! 2! 3! 4!]',
    'a:1!', 'a:1!,b:2!', 'a:1!,b:2!,c:3!', 'a:1!,b:2!,c:3!,d:4!',
    'a:1! b:2!', 'a:1! b:2! c:3!', 'a:1! b:2! c:3!,d:4!',
    '{a:1!}', '{a:1!,b:2!}', '{a:1!,b:2!,c:3!}', '{a:1!,b:2!,c:3!,d:4!}',
    '{a:1! b:2!}', '{a:1! b:2! c:3!}', '{a:1! b:2! c:3! d:4!}',
  ]
  for (const c of cases) {
    entries.push([c, j(c)])
  }
  writeTsv('unary-suffix-structure.tsv', 'Unary suffix in structures - config:suffix', entries)
}

// === unary-suffix-prefix ===
{
  const je = Jsonic.make().use(Expr, {
    op: {
      factorial: { suffix: true, left: 18000, src: '!' },
      question: { suffix: true, left: 14000, src: '?' },
    }
  })
  const j = mj(je)
  const entries = []
  const cases = [
    '-1!', '--1!', '-1!!', '--1!!',
    '-1!+2', '--1!+2', '---1!+2',
    '-1?', '--1?', '-1??', '--1??',
    '-1!?', '-1!?!',
    '-1?+2', '--1?+2',
  ]
  for (const c of cases) {
    entries.push([c, j(c)])
  }
  writeTsv('unary-suffix-prefix.tsv', 'Combined suffix and prefix - config:suffix', entries)
}

// === unary-prefix-edge ===
{
  const je = Jsonic.make().use(Expr, {
    op: {
      at: { prefix: true, right: 16000, src: '@' },
      tight: { infix: true, left: 19000, right: 19010, src: '~' },
    }
  })
  const j = mj(je)
  const entries = []
  const cases = [
    '@1', '@@1', '@@@1',
    '-@1', '@-1', '--@1', '@--1',
    '@@-1', '-@@1', '-@-1', '@-@1',
    '@1+2', '1+@2', '@1+@2',
    '@1+2+3', '1+@2+3', '@1+@2+3',
    '@1+2+@3', '1+@2+@3', '@1+@2+@3',
    '@1~2', '1~@2', '@1~@2',
    '@1~2+3', '1~@2+3', '@1~@2+3',
    '@1~2~3', '1~@2~3', '@1~@2~3',
  ]
  for (const c of cases) {
    entries.push([c, j(c)])
  }
  writeTsv('unary-prefix-edge.tsv', 'Unary prefix edge cases - config:prefix-tight', entries)
}

// === ternary-basic ===
{
  const je = Jsonic.make().use(Expr, {
    op: {
      factorial: { suffix: true, src: '!', left: 18000 },
      ternary: { ternary: true, src: ['?', ':'] },
    }
  })
  const j = mj(je)
  const entries = []
  const cases = [
    '1?2:3',
    '1?2: 3?4:5', '1?4:5 ?2:3',
    '1? 2?4:5 :3',
    '0+1?2:3',
    '0+1?2: 3?4:5', '0+1?4:5 ?2:3',
    '0+1? 2?4:5 :3',
    '1?0+2:3',
    '1?2:0+3',
    '0+1?0+2:0+3',
    '-1?2:3',
    '1!?2:3',
    '-1!?2:3',
  ]
  for (const c of cases) {
    entries.push([c, j(c)])
  }
  writeTsv('ternary-basic.tsv', 'Ternary operator tests - config:ternary', entries)
}

// === json-base ===
{
  const j = mj(Jsonic.make().use(Expr))
  const entries = []
  const scalars = [
    ['1', j('1')],
    ['"a"', j('"a"')],
    ['true', j('true')],
  ]
  entries.push(...scalars)
  writeTsv('json-base.tsv', 'JSON base compatibility - default Expr config', entries)
}

// === paren-suffix ===
{
  const je = Jsonic.make().use(Expr, {
    op: {
      factorial: { suffix: true, left: 18000, src: '!' },
      question: { suffix: true, left: 14000, src: '?' },
    }
  })
  const j = mj(je)
  const entries = []
  const cases = [
    '(0!+1!+2!)', '(0!+1!+2)', '(0!+1+2!)', '(0!+1+2)',
  ]
  for (const c of cases) {
    entries.push([c, j(c)])
  }
  writeTsv('unary-suffix-paren.tsv', 'Suffix operators inside parens - config:suffix', entries)
}

console.log('Done generating spec files!')
