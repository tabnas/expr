import Jsonic from '@jsonic/jsonic-next'
import { Expr } from '../dist/expr.js'
import { writeFileSync } from 'fs'

function gen(name, inputs, j) {
  const lines = [`# ${name}\n# input\texpected_output`]
  for (const input of inputs) {
    try {
      const result = j(input)
      lines.push(`${input}\t${JSON.stringify(result)}`)
    } catch (e) {
      console.error(`ERROR: ${name}: ${input}: ${e.message}`)
    }
  }
  writeFileSync(`test/spec/${name}.tsv`, lines.join('\n') + '\n')
  console.log(`Wrote ${name}.tsv (${inputs.length} cases)`)
}

const je_overload = Jsonic.make().use(Expr, {
  op: {
    factorial: { suffix: true, left: 15000, src: '!' },
    square: { osrc: '[', csrc: ']', paren: true, preval: { required: true } },
    brace: { osrc: '{', csrc: '}', paren: true, preval: { required: true } }
  }
})
const jo = (s) => je_overload(s)
gen('paren-preval-overload', [
  '[1]', 'a[1]', '[a[1]]', 'a:[1]', 'a:b[1]', 'a:[b[1]]',
  '{a:[1]}', '{a:b[1]}', '{a:[b[1]]}',
  '-[1]+2', '-a[1]+2', '-[a[1]]+2',
  '2+[1]', '2+a[1]', '2+[a[1]]',
  '2+{a:[1]}', '2+{a:b[1]}', '2+{a:[b[1]]}',
  'a[b[1]]', 'a[b[c[1]]]', 'a[b[c[d[1]]]]',
  'a{1}', 'a{b{1}}', 'a{b{c{1}}}',
  'a{1+2}', 'a{b{1+2}}', 'a{b{c{1+2}}}',
  'a{{x:1}}', 'a{{x:1,y:2}}',
], jo)

const je_impl = Jsonic.make().use(Expr, {
  op: { plain: { preval: true } }
})
const ji = (s) => je_impl(s)
gen('paren-preval-implicit', [
  'foo,(1,a)', 'foo,(1+2,a)', 'foo,(1+2+3,a)',
], ji)

console.log('Done!')
