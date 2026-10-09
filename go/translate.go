package tabnasexpr

import _ "embed"

// TranslationPart is one optional alchemy source and the entry point a host calls.
type TranslationPart struct {
	Entry  string
	Source string
}

// TranslationParts is the package-local structural translation interface.
type TranslationParts struct {
	Manifest string
	Lift     *TranslationPart
	// Embed is an optional embedding of a plain tree in the format's
	// schema, with its reverse.
	Embed  *TranslationPart
	Render *TranslationPart
}

// The copies ../ts/embed-translate.js writes (npm run embed), since a Go
// module embeds only files inside it.

//go:embed translate/manifest.json
var translationManifest string

//go:embed translate/embed.alc
var translationEmbed string

//go:embed translate/render.alc
var translationRender string

var translationParts = TranslationParts{
	Manifest: translationManifest,
	Embed:    &TranslationPart{Entry: "expr-embed", Source: translationEmbed},
	Render:   &TranslationPart{Entry: "expr-render", Source: translationRender},
}

// Translate returns the expression format's immutable translation parts.
func Translate() *TranslationParts { return &translationParts }
