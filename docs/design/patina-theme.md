# Patina colour system

The palette is authored for Patina: ink surfaces with restrained mineral
accents and a pale blue focus marker. It uses no reference application
assets or sampled colours. Existing colour names remain settings identifiers;
their RGB values are replaced. Guide geometry is outside this change.

Programme and list focus uses opaque pale blue with ink text. Controls with
white labels use an opaque dark accent with a light 2 dp focus outline. Progress tracks, focus borders and
playing markers use the light accent (dark on a focused blue row).
The now line retains its 2 dp total width, split into adjacent 1 dp light
and dark strokes visible over ink, the lit row and blue focus. Focused programme cells stay opaque even in overlay mode. Secondary text is white at 77% opacity.
The dark surfaces limit glare. The fixed pale-blue fill distinguishes focus
from the smaller playback and time indicators. Focus corners retain the existing 6 dp radius. Fonts, type sizes,
spacing, density, icons and layout remain as before.

Automated contrast checks cover every saved accent/background pairing:
primary and secondary text at least 4.5:1, focus text at least 7:1, and
focus outlines/surfaces, now markers and progress indicators at least 3:1 against
their respective surfaces. Opaque focused cells and control fills isolate
label contrast from changing video. These numeric checks complement device
inspection; they do not establish screen-reader accessibility or viewing-
distance legibility.

## TV validation (2026-11-12)

Built the x86_64 Android host and ran it on the API 36 `tv36` emulator
(`emulator-5556`, 1920×1080). Imported 24 generated channels/guide entries
from `tools/make-iptv-fixtures.py`, with `tools/make-hls-fixtures.sh` video
served by `tools/mock-server.py`. No provider data was used.

Inspected programme/list focus, the now line crossing focused and resting
cells, progress bars, and the control outline over moving colour bars.
The guide's surface steps remain distinguishable; the muted background
variants intentionally keep saturation low. The run found an unbacked
playback hint over bright video: it now uses the same dark translucent
panel as the controls. Numeric contrast tests also cover those panels
composited over black and white video extremes.

- [Guide with playing synthetic preview](evidence/patina-tv-guide.png)
- [Playback overlay](evidence/patina-tv-playback.png)
- [Focused controls over video](evidence/patina-tv-controls.png)

This is TV-emulator validation, not a claim about a physical television's
black levels, calibration or viewing-distance legibility.

The pale-blue focus (`#C8DDEB`) keeps the selection consistent with the
default blue indicators while remaining a filled, dark-text selection.
Other accent preferences retain their chosen indicator colour.
The guide/list type, density and spacing are unchanged. The two-tone now
line has the original 2 dp outer width, checked by a rendered layout test.
[Focused All channels group](evidence/patina-tv-groups.png) uses the same fill.
