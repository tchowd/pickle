# Mac source-window OCR verification

The old development bundle failed the real controlled source-window test: selection
capture preserved the exact passage, the GUI reported Accessibility and Screen
Recording access, and the retained source image appeared reduced into roughly the
left 55–60% of a wider white canvas. OCR returned an empty string. The independent
source screenshot was crisp and measured 1400×778 pixels for a 700×389-point window.
That observed failure must not be reported as working source-window OCR. A shell
process's separate permission result does not establish the running GUI's status.

Run the content-free native fixture from `rune`:

```sh
bash native/macos/tests/run-page-fixture.sh
```

The script compiles a separate test executable for the macOS 14 deployment baseline.
It does not replace, rebuild, or launch the installed app bundle. It creates no
application, windows, screen capture, permissions request, image file, or network
request. CoreText draws fixed text and edge markers into owned in-memory bitmaps.
The production `processPage` function performs actual Vision recognition and JPEG
encoding. The test requires both large and ordinary body text, checks RGB/BGRA input,
and verifies JPEG dimensions and a right-edge marker. Its output contains only
fixed-fixture result flags and numeric configuration metadata.

The enlarged-fixture case also exercises retained-JPEG downscaling. It exposed a
one-pixel floating-point truncation, now corrected by rounding dimensions. Its
right edge was preserved even before that correction; this small rounding issue
does not explain the much larger white-padding defect observed in the GUI.

Before changing the capture configuration, that production processing path already
recognized the fixed 1400×778 image and preserved the right edge. On this machine,
the inert configuration defaults were `scalesToFit=false`, automatic capture
resolution, ignored single-window shadows, and Vision minimum text height zero.
This provides evidence for a working controlled recognizer/compressor, independently
of the failed real ScreenCaptureKit image. It does not prove the reason for that
real failure.

The capture configuration now uses `SCContentFilter.contentRect` in points multiplied
by that filter's `pointPixelScale`, requests the best available source resolution,
and keeps proportional output within a 2000-pixel side limit. It never guesses that
every source uses 2× backing pixels. It explicitly preserves aspect ratio, omits
window shadows/cursor, and requests SDR-compatible BGRA/sRGB output. `scalesToFit`
stays false: dimensions are derived from actual source pixels, with downscaling only
when the bound requires it. Vision's minimum text height is explicitly zero rather
than depending on an OS default. The fixture covers Retina/1× geometry, bounded
downscaling, and rejection of invalid geometry without capturing anything.

The actual repaired pipeline still requires a new same-process GUI test after the
parent agent's build/checkpoint: select the controlled source, confirm its retained
image fills the expected bounds sharply, and verify nonempty recognized fixture text.
Test different backing scales, window sizes, and source apps separately. Do not send
real passages or screenshots to a provider while diagnosing this path.

Primary Apple references (all configuration properties used here are available at
the macOS 14 baseline):

- [Filter content rectangle, in screen points](https://developer.apple.com/documentation/screencapturekit/sccontentfilter/contentrect)
- [Point-to-pixel scale](https://developer.apple.com/documentation/screencapturekit/sccontentfilter/pointpixelscale)
- [Capture resolution](https://developer.apple.com/documentation/screencapturekit/scstreamconfiguration/captureresolution)
- [Best resolution](https://developer.apple.com/documentation/screencapturekit/sccaptureresolutiontype/best)
- [Independent-window scaling](https://developer.apple.com/documentation/screencapturekit/scstreamconfiguration/scalestofit)
- [Single-window shadows](https://developer.apple.com/documentation/screencapturekit/scstreamconfiguration/ignoreshadowssinglewindow)
- [Source rectangle is ignored for a single window](https://developer.apple.com/documentation/screencapturekit/scstreamconfiguration/sourcerect)
- [Destination rectangle is ignored for a single window](https://developer.apple.com/documentation/screencapturekit/scstreamconfiguration/destinationrect)
- [Vision minimum text height](https://developer.apple.com/documentation/vision/vnrecognizetextrequest/minimumtextheight)
- [Vision recognition pipeline](https://developer.apple.com/documentation/vision/recognizing-text-in-images)

Apple's public minimum-text-height page describes a 1/32 default; the installed newer
SDK header and measured runtime default are zero. Explicit zero requests the highest
available recognition resolution within this pipeline's bounded input dimensions.
