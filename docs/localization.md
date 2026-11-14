# Interface languages

Choose **Settings → General → Language**. The choice applies immediately and
is saved in settings and backups. **System language** uses the device's preferred
locale when the app starts. Unsupported device languages fall back to English.

The initial catalogue provides English, Spanish, French, German, Italian,
Portuguese, Russian, Simplified Chinese, Traditional Chinese, Japanese and Korean.
Regional variants use the base language (for example `es-MX` and `pt-BR`). Chinese
script tags take precedence over regions: `zh-Hant` uses Traditional Chinese;
`zh-TW`, `zh-HK` and `zh-MO` also use Traditional Chinese unless `Hans` is explicit.

Coverage includes navigation, settings pages and active setting labels, common
buttons, playlist setup fields, player controls and common playback errors.
Long-form help, some secondary screens/status messages, track language names,
pluralized counts and date/time formatting still use the existing English text.
Missing messages always retain their English source. Provider channel names,
playlist names, programme titles and descriptions are never translated.

## Maintaining translations

The portable lookup and language resolution live in
`crates/iptv-core/src/i18n.rs`; platform locale discovery is confined to
`crates/iptv-app/src/i18n.rs`. `sys-locale` supplies the native device locale.
No network service is involved in translating messages.

`crates/iptv-core/src/i18n/messages.tsv` is UTF-8 with one message per line.
Columns follow `LANGUAGES` in `i18n.rs`: English source, Spanish, French, German,
Italian, Portuguese, Russian, Simplified Chinese, Traditional Chinese, Japanese,
Korean. Each row must have every column, with no empty translations or duplicate
source messages. Tabs and newlines cannot appear inside messages.

Use `tr("English source")` only for app-owned messages. Call it inside a reactive
closure when text must change while the view remains mounted. Never apply it to
arbitrary text in a shared text renderer: that could translate provider content.
Adding a language also requires adding its tag to
`iptv_settings::catalogue::choices::LANGUAGE`; a test checks these lists agree.
The setting remains text-valued so older backups and unlisted locale tags survive.

Run `cargo test -p iptv-core -p iptv-settings`, the core wasm build, and the app
build. The `interface_languages` headless scenario checks switching languages
without remounting settings, persistence, and returning to the system preference.
Headless tests pin their system locale through `IPTV_RS_TEST_LOCALE`, available
only in test builds.
