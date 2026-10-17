// Prints what macOS ImageIO (Finder, Preview, Photos) reads from a file's metadata.
import Foundation
import ImageIO

let url = URL(fileURLWithPath: CommandLine.arguments[1])
guard let source = CGImageSourceCreateWithURL(url as CFURL, nil),
      let props = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [String: Any] else {
    print("{\"error\": \"unreadable\"}")
    exit(0)
}
let exif = props[kCGImagePropertyExifDictionary as String] as? [String: Any] ?? [:]
let tiff = props[kCGImagePropertyTIFFDictionary as String] as? [String: Any] ?? [:]
let gps = props[kCGImagePropertyGPSDictionary as String] as? [String: Any] ?? [:]
let out: [String: Any] = [
    "orientation": props[kCGImagePropertyOrientation as String] ?? NSNull(),
    "make": tiff[kCGImagePropertyTIFFMake as String] ?? NSNull(),
    "exif_keys": exif.count,
    "gps_keys": gps.count,
    "lens_model": exif[kCGImagePropertyExifLensModel as String] ?? NSNull(),
]
let data = try JSONSerialization.data(withJSONObject: out, options: [.sortedKeys])
print(String(data: data, encoding: .utf8)!)
