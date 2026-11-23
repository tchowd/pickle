import AppKit
import CoreText
import ImageIO
import ScreenCaptureKit
import Vision
import Darwin

private enum FixtureError: Error { case failed(String) }

@main struct PageFixture {
  static func main() {
    do { try run() }
    catch FixtureError.failed(let message) {
      let data = (try? JSONSerialization.data(withJSONObject: ["ok": false, "error": message]))
        ?? Data("{\"ok\":false,\"error\":\"Controlled native fixture failed.\"}".utf8)
      print(String(decoding: data, as: UTF8.self))
      exit(1)
    } catch {
      print("{\"ok\":false,\"error\":\"Controlled native fixture failed.\"}")
      exit(1)
    }
  }
  static func run() throws {
    let retina = try pageCaptureConfiguration(
      contentRect: CGRect(x: 300, y: 120, width: 700, height: 389), pointPixelScale: 2)
    let standard = try pageCaptureConfiguration(
      contentRect: CGRect(x: -1400, y: 0, width: 700, height: 389), pointPixelScale: 1)
    let capped = try pageCaptureConfiguration(
      contentRect: CGRect(x: 0, y: 0, width: 2560, height: 1440), pointPixelScale: 2)
    guard retina.width == 1400, retina.height == 778,
      standard.width == 700, standard.height == 389,
      capped.width == 2000, capped.height == 1125,
      retina.captureResolution == .best, !retina.scalesToFit,
      retina.preservesAspectRatio, retina.ignoreShadowsSingleWindow,
      !retina.showsCursor, retina.colorSpaceName == CGColorSpace.sRGB
    else { throw FixtureError.failed("Capture configuration changed source pixel geometry or policy.") }
    for (rectangle, scale) in [
      (CGRect.zero, Float(2)), (CGRect(x: 0, y: 0, width: 700, height: 389), Float.nan),
      (CGRect(x: 0, y: 0, width: 700, height: 389), Float(0)),
    ] {
      do {
        _ = try pageCaptureConfiguration(contentRect: rectangle, pointPixelScale: scale)
        throw FixtureError.failed("Invalid capture geometry was accepted.")
      } catch PageCaptureError.invalidGeometry { }
    }
    let width = 1400
    let height = 778
    guard let canvas = CGContext(
      data: nil, width: width, height: height, bitsPerComponent: 8, bytesPerRow: width * 4,
      space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue)
    else { throw FixtureError.failed("Could not allocate the controlled fixture.") }
    canvas.setFillColor(CGColor(gray: 1, alpha: 1))
    canvas.fill(CGRect(x: 0, y: 0, width: width, height: height))
    // Edge markers detect a right-side padding regression in the retained JPEG.
    canvas.setFillColor(CGColor(red: 1, green: 0, blue: 0, alpha: 1))
    canvas.fill(CGRect(x: 0, y: 0, width: 12, height: height))
    canvas.setFillColor(CGColor(red: 0, green: 0, blue: 1, alpha: 1))
    canvas.fill(CGRect(x: width - 12, y: 0, width: 12, height: height))
    let phrases: [(String, CGFloat, CGFloat)] = [
      ("PICKLE CONTROLLED FIXTURE", 46, 620),
      ("Reading this image requires no screen access.", 28, 520),
      ("The treatment may reduce symptoms in some patients.", 22, 420),
      ("Larger studies are needed before general use.", 22, 365),
    ]
    for (phrase, size, baseline) in phrases {
      let attributes: [NSAttributedString.Key: Any] = [
        NSAttributedString.Key(kCTFontAttributeName as String): CTFontCreateWithName("Helvetica" as CFString, size, nil),
        NSAttributedString.Key(kCTForegroundColorAttributeName as String): CGColor(gray: 0, alpha: 1),
      ]
      canvas.textPosition = CGPoint(x: 50, y: baseline)
      CTLineDraw(CTLineCreateWithAttributedString(NSAttributedString(string: phrase, attributes: attributes)), canvas)
    }
    guard let image = canvas.makeImage() else { throw FixtureError.failed("Could not create the controlled fixture.") }
    let result = try processPage(image)
    // The real AppKit fixture uses white text on a dark background. Check that
    // contrast direction independently of SCK sizing and capture permission.
    canvas.setBlendMode(.difference)
    canvas.setFillColor(CGColor(gray: 1, alpha: 1))
    canvas.fill(CGRect(x: 0, y: 0, width: width, height: height))
    guard let darkImage = canvas.makeImage(),
      let darkText = try processPage(darkImage)["text"] as? String,
      darkText.uppercased().contains("PICKLE"), darkText.lowercased().contains("symptoms")
    else { throw FixtureError.failed("Production Vision processing missed controlled white text on a dark background.") }
    guard let bgraCanvas = CGContext(
      data: nil, width: width, height: height, bitsPerComponent: 8, bytesPerRow: width * 4,
      space: CGColorSpace(name: CGColorSpace.sRGB)!,
      bitmapInfo: CGBitmapInfo.byteOrder32Little.rawValue | CGImageAlphaInfo.premultipliedFirst.rawValue)
    else { throw FixtureError.failed("Could not allocate the controlled BGRA fixture.") }
    bgraCanvas.draw(image, in: CGRect(x: 0, y: 0, width: width, height: height))
    guard let bgraImage = bgraCanvas.makeImage(),
      let bgraText = try processPage(bgraImage)["text"] as? String,
      bgraText.uppercased().contains("PICKLE"), bgraText.lowercased().contains("symptoms")
    else { throw FixtureError.failed("Production Vision processing missed the controlled BGRA fixture.") }
    guard let text = result["text"] as? String,
      text.uppercased().contains("PICKLE"), text.uppercased().contains("CONTROLLED"),
      let jpeg = result["jpeg"] as? [UInt8],
      let source = CGImageSourceCreateWithData(Data(jpeg) as CFData, nil),
      let retained = CGImageSourceCreateImageAtIndex(source, 0, nil)
    else { throw FixtureError.failed("Production Vision processing did not recognize the controlled fixture.") }
    guard retained.width == width, retained.height == height,
      let right = NSBitmapImageRep(cgImage: retained).colorAt(x: width - 6, y: height / 2)?.usingColorSpace(.deviceRGB),
      right.blueComponent > 0.6, right.redComponent < 0.3
    else { throw FixtureError.failed("Production JPEG processing changed the controlled image bounds or padded its right edge.") }
    guard let largeCanvas = CGContext(
      data: nil, width: width * 2, height: height * 2, bitsPerComponent: 8,
      bytesPerRow: width * 2 * 4, space: CGColorSpaceCreateDeviceRGB(),
      bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue)
    else { throw FixtureError.failed("Could not allocate the controlled resizing fixture.") }
    largeCanvas.draw(image, in: CGRect(x: 0, y: 0, width: width * 2, height: height * 2))
    guard let largeImage = largeCanvas.makeImage(),
      let largeJPEG = try processPage(largeImage)["jpeg"] as? [UInt8],
      let largeSource = CGImageSourceCreateWithData(Data(largeJPEG) as CFData, nil),
      let reduced = CGImageSourceCreateImageAtIndex(largeSource, 0, nil),
      let reducedRight = NSBitmapImageRep(cgImage: reduced).colorAt(x: reduced.width - 6, y: reduced.height / 2)?.usingColorSpace(.deviceRGB)
    else { throw FixtureError.failed("Production JPEG resizing could not process the controlled image.") }
    guard reduced.width == 1440, reduced.height == 800,
      reducedRight.blueComponent > 0.6, reducedRight.redComponent < 0.3
    else {
      let dimensions: [String: Any] = ["resize_width": reduced.width, "resize_height": reduced.height,
        "resize_right_blue": reducedRight.blueComponent, "resize_right_red": reducedRight.redComponent]
      print(String(decoding: try JSONSerialization.data(withJSONObject: dimensions), as: UTF8.self))
      throw FixtureError.failed("Production JPEG resizing padded or distorted the controlled image.")
    }
    let defaults = SCStreamConfiguration()
    let report: [String: Any] = [
      "controlled_fixture_recognized": true,
      "controlled_dark_fixture_recognized": true,
      "controlled_bgra_fixture_recognized": true,
      "capture_geometry_cases_passed": 6,
      "configured_capture_resolution": retina.captureResolution.rawValue,
      "configured_pixel_format": retina.pixelFormat,
      "retina_output_width": retina.width, "retina_output_height": retina.height,
      "standard_output_width": standard.width, "standard_output_height": standard.height,
      "fixture_width": width, "fixture_height": height,
      "retained_width": retained.width, "retained_height": retained.height,
      "right_edge_preserved": true,
      "resized_right_edge_preserved": true,
      "default_scales_to_fit": defaults.scalesToFit,
      "default_capture_resolution": defaults.captureResolution.rawValue,
      "default_ignore_shadows": defaults.ignoreShadowsSingleWindow,
      "default_minimum_text_height": VNRecognizeTextRequest().minimumTextHeight,
      "controlled_body_text_recognized": text.lowercased().contains("symptoms") && text.lowercased().contains("larger"),
    ]
    let data = try JSONSerialization.data(withJSONObject: report, options: [.sortedKeys])
    print(String(decoding: data, as: UTF8.self))
    guard text.lowercased().contains("symptoms"), text.lowercased().contains("larger")
    else { throw FixtureError.failed("Production Vision processing missed the controlled body text.") }
  }
}
