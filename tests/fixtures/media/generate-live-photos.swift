// Procedural local test resources. Never reads Photos, camera, microphone or user media.
import AVFoundation
import CoreVideo
import Foundation
import ImageIO
import UniformTypeIdentifiers

let root = URL(fileURLWithPath: CommandLine.arguments[1], isDirectory: true)
guard try FileManager.default.contentsOfDirectory(atPath: root.path).isEmpty else { fatalError("Use an empty owned temporary directory") }
let width = 640, height = 480
func image(_ name: String, _ identifier: String?) throws {
    let context = CGContext(data: nil, width: width, height: height, bitsPerComponent: 8, bytesPerRow: width * 4, space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
    context.setFillColor(CGColor(red: 0.2, green: 0.6, blue: 0.8, alpha: 1)); context.fill(CGRect(x: 0, y: 0, width: width, height: height))
    let out = CGImageDestinationCreateWithURL(root.appendingPathComponent(name).appendingPathExtension("jpg") as CFURL, UTType.jpeg.identifier as CFString, 1, nil)!
    var properties: [CFString: Any] = [:]
    if let identifier = identifier { properties[kCGImagePropertyMakerAppleDictionary] = ["17": identifier] }
    CGImageDestinationAddImage(out, context.makeImage()!, properties as CFDictionary)
    guard CGImageDestinationFinalize(out) else { fatalError("JPEG failed") }
}
func movie(_ name: String, _ identifier: String?) throws {
    let writer = try AVAssetWriter(outputURL: root.appendingPathComponent(name).appendingPathExtension("mov"), fileType: .mov)
    let input = AVAssetWriterInput(mediaType: .video, outputSettings: [AVVideoCodecKey: AVVideoCodecType.h264, AVVideoWidthKey: width, AVVideoHeightKey: height])
    let pixels = AVAssetWriterInputPixelBufferAdaptor(assetWriterInput: input, sourcePixelBufferAttributes: [kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_32ARGB, kCVPixelBufferWidthKey as String: width, kCVPixelBufferHeightKey as String: height])
    writer.add(input)
    var timed: AVAssetWriterInput?
    var adaptor: AVAssetWriterInputMetadataAdaptor?
    if let identifier = identifier {
        let content = AVMutableMetadataItem(); content.identifier = .quickTimeMetadataContentIdentifier; content.value = identifier as NSString; content.dataType = kCMMetadataBaseDataType_UTF8 as String
        writer.metadata = [content]
        var description: CMFormatDescription?
        let specifications: [[String: Any]] = [[kCMMetadataFormatDescriptionMetadataSpecificationKey_Identifier as String: "mdta/com.apple.quicktime.still-image-time", kCMMetadataFormatDescriptionMetadataSpecificationKey_DataType as String: "com.apple.metadata.datatype.int8"]]
        guard CMMetadataFormatDescriptionCreateWithMetadataSpecifications(allocator: kCFAllocatorDefault, metadataType: kCMMetadataFormatType_Boxed, metadataSpecifications: specifications as CFArray, formatDescriptionOut: &description) == noErr else { fatalError("Metadata description") }
        timed = AVAssetWriterInput(mediaType: .metadata, outputSettings: nil, sourceFormatHint: description)
        writer.add(timed!); adaptor = AVAssetWriterInputMetadataAdaptor(assetWriterInput: timed!)
    }
    guard writer.startWriting() else { fatalError("Start") }; writer.startSession(atSourceTime: .zero)
    if let adaptor = adaptor {
        let item = AVMutableMetadataItem(); item.identifier = AVMetadataIdentifier(rawValue: "mdta/com.apple.quicktime.still-image-time"); item.value = NSNumber(value: Int8(-1)); item.dataType = "com.apple.metadata.datatype.int8"
        guard adaptor.append(AVTimedMetadataGroup(items: [item], timeRange: CMTimeRange(start: CMTime(value: 10, timescale: 20), duration: CMTime(value: 1, timescale: 600)))) else { fatalError("Timed metadata") }
        timed!.markAsFinished()
    }
    for frame in 0..<30 {
        let deadline = Date().addingTimeInterval(5)
        while !input.isReadyForMoreMediaData { guard Date() < deadline else { fatalError("Frame wait") }; Thread.sleep(forTimeInterval: 0.005) }
        var pixel: CVPixelBuffer?
        guard CVPixelBufferCreate(kCFAllocatorDefault, width, height, kCVPixelFormatType_32ARGB, nil, &pixel) == kCVReturnSuccess, let pixel = pixel else { fatalError("Pixel") }
        CVPixelBufferLockBaseAddress(pixel, [])
        let base = CVPixelBufferGetBaseAddress(pixel)!.assumingMemoryBound(to: UInt8.self), stride = CVPixelBufferGetBytesPerRow(pixel)
        for y in 0..<height { for x in 0..<width { let n = y * stride + x * 4; base[n] = 255; base[n+1] = UInt8(50 + frame * 4); base[n+2] = UInt8(x * 255 / width); base[n+3] = UInt8(y * 255 / height) } }
        CVPixelBufferUnlockBaseAddress(pixel, [])
        guard pixels.append(pixel, withPresentationTime: CMTime(value: Int64(frame), timescale: 20)) else { fatalError("Frame append") }
    }
    input.markAsFinished(); let finished = DispatchSemaphore(value: 0); writer.finishWriting { finished.signal() }
    guard finished.wait(timeout: .now() + 15) == .success, writer.status == .completed else { fatalError("Finish") }
}
for name in ["PairA", "PairB", "Mismatch", "Ordinary"] {
    let identifier = name == "Ordinary" ? nil : UUID().uuidString
    try image(name, identifier)
    try movie(name, name == "Mismatch" ? UUID().uuidString : identifier)
}
print("Generated four owned photo/movie candidates, including mismatch and ordinary files")
