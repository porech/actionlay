// Verify Finder's alias after the final DMG has been compressed and remounted.
import Foundation
import CoreFoundation
import ImageIO

do {
    let record = try Data(contentsOf: URL(fileURLWithPath: CommandLine.arguments[1]))
    // DS_Store's backgroundImageAlias is a legacy Carbon alias record.
    guard let bookmark = CFURLCreateBookmarkDataFromAliasRecord(nil, record as CFData)?.takeRetainedValue() else {
        throw NSError(domain: "ActionLayDMG", code: 1)
    }
    var stale = false
    let resolved = try URL(resolvingBookmarkData: bookmark as Data,
                           options: [.withoutUI, .withoutMounting], relativeTo: nil,
                           bookmarkDataIsStale: &stale)
    let expected = URL(fileURLWithPath: CommandLine.arguments[2]).resolvingSymlinksInPath()
    guard resolved.resolvingSymlinksInPath() == expected,
          let image = CGImageSourceCreateWithURL(resolved as CFURL, nil),
          let properties = CGImageSourceCopyPropertiesAtIndex(image, 0, nil) as? [CFString: Any],
          properties[kCGImagePropertyPixelWidth] as? Int == 720,
          properties[kCGImagePropertyPixelHeight] as? Int == 480 else {
        throw NSError(domain: "ActionLayDMG", code: 2,
                      userInfo: [NSLocalizedDescriptionKey: "Finder background does not resolve to the 720×480 artwork"])
    }
    print("Verified Finder background: \(resolved.path)")
} catch {
    fputs("DMG background verification failed: \(error)\n", stderr)
    exit(1)
}
