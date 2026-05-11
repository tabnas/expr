const { Jsonic, Debug, util } = require('jsonic')

const { Expr } = require('..')

// Walk expr tree into simplified form where first element is the op src.
const S = (x) =>
  x && Array.isArray(x)
    ? 0 === x.length
      ? x
      : [
          x[0].src || S(x[0]),
          ...(1 < x.length ? x.slice(1).map((t) => S(t)) : []),
        ].filter((t) => undefined !== t)
    : null != x && 'object' === typeof x
      ? util.omap(x, ([n, v]) => [n, S(v)])
      : x

const clean = (v) => JSON.parse(JSON.stringify(v))

const j = Jsonic.make({
  debug: {
    print: {
      config: true,
      src: (x) => JSON.stringify(S(x)),
    },
  },
})
  .use(Debug)
  .use(Expr, {
    op: {
      // factorial: {
      //   suffix: true, left: 15000, src: '!'
      // },
      // question: {
      //   suffix: true, left: 13000, src: '?'
      // }

      // question: {
      //   infix: true, left: 15, right: 14, src: '?'
      // },
      // semicolon: {
      //   infix: true, left: 16, right: 17, src: ';'
      // },

      // foo: {
      //   ternary: true,
      //   src: ['?', ':'],
      // },
      // bar: {
      //   ternary: true,
      //   src: ['QQ', 'CC'],
      // },
      // zed: {
      //   ternary: true,
      //   src: ['%%', '@@'],
      // },

      // plain: {
      //    preval: {},
      // },

      // angle: {
      //   osrc: '<',
      //   csrc: '>',
      //   // preval: { required: true },
      //   postval: {},
      // },
      square: {
        osrc: '[',
        csrc: ']',
        paren: true,
        preval: {
          required: true,
        },
      },
      // brace: {
      //   osrc: '{',
      //   csrc: '}',
      //   preval: {
      //     required: true
      //   },
      // }

      // ternary: {
      //   osrc: '?',
      //   csrc: ';',
      //   // preval: { required: true },
      //   // postval: { required: true },
      // }
    },
  })

console.log(j.debug.describe())
// , {
//   op: {
//     square: {
//       order: 2, bp: [100410, 100400], src: '[', close: ']'
//     }
//   }
// })

// console.dir(j.rule('elem').def, {depth:null})

const v = j(process.argv[2], { log: -1 })
// console.log(v)
console.log(S(v))
//console.log(clean(v), '###', v)
console.dir(clean(S(v)), { depth: null })
