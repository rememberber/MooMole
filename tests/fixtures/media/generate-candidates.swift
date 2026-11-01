// Test-only procedural video. No user media, microphone, camera or network.
// The caller owns an empty automatic-cleanup temporary directory.
import AVFoundation
import CoreVideo
import Foundation

let folder = URL(fileURLWithPath: CommandLine.arguments[1], isDirectory: true)
guard FileManager.default.fileExists(atPath: folder.path),
      try FileManager.default.contentsOfDirectory(atPath: folder.path).isEmpty else {
    fatalError("Use an empty owned temporary directory")
}
for (name, scale, other, count) in [
    ("reference.mp4", 1, false, 60),
    ("resized.mp4", 2, false, 60),
    ("different.mp4", 1, true, 60),
    ("longer.mp4", 1, false, 100),
] {
    let width = 160 * scale, height = 96 * scale
    let writer = try AVAssetWriter(outputURL: folder.appendingPathComponent(name), fileType: .mp4)
    let input = AVAssetWriterInput(mediaType: .video, outputSettings: [
        AVVideoCodecKey: AVVideoCodecType.h264,
        AVVideoWidthKey: width, AVVideoHeightKey: height,
        AVVideoCompressionPropertiesKey: [AVVideoMaxKeyFrameIntervalKey: 20],
    ])
    let adaptor = AVAssetWriterInputPixelBufferAdaptor(assetWriterInput: input, sourcePixelBufferAttributes: [
        kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_32ARGB,
        kCVPixelBufferWidthKey as String: width, kCVPixelBufferHeightKey as String: height,
    ])
    writer.add(input)
    guard writer.startWriting() else { fatalError("Writer failed") }
    writer.startSession(atSourceTime: .zero)
    for frame in 0..<count {
        let deadline = Date().addingTimeInterval(5)
        while !input.isReadyForMoreMediaData {
            guard Date() < deadline else { fatalError("Frame deadline") }
            Thread.sleep(forTimeInterval: 0.005)
        }
        var pixel: CVPixelBuffer?
        guard CVPixelBufferCreate(kCFAllocatorDefault, width, height, kCVPixelFormatType_32ARGB, nil, &pixel) == kCVReturnSuccess,
              let pixel = pixel else { fatalError("Pixel buffer failed") }
        CVPixelBufferLockBaseAddress(pixel, [])
        let base = CVPixelBufferGetBaseAddress(pixel)!.assumingMemoryBound(to: UInt8.self)
        let stride = CVPixelBufferGetBytesPerRow(pixel)
        for y in 0..<height {
            for x in 0..<width {
                let offset = y * stride + x * 4
                base[offset] = 255
                if other {
                    let light: UInt8 = ((x / (8 * scale) + y / (8 * scale) + frame / 10) % 2 == 0) ? 230 : 20
                    base[offset + 1] = light; base[offset + 2] = light; base[offset + 3] = light
                } else {
                    base[offset + 1] = UInt8(50 + (frame * 150 / (count - 1)))
                    base[offset + 2] = UInt8(x * 255 / (width - 1))
                    base[offset + 3] = UInt8(y * 255 / (height - 1))
                }
            }
        }
        CVPixelBufferUnlockBaseAddress(pixel, [])
        guard adaptor.append(pixel, withPresentationTime: CMTime(value: Int64(frame), timescale: 20)) else {
            fatalError("Append failed")
        }
    }
    input.markAsFinished()
    let finished = DispatchSemaphore(value: 0)
    writer.finishWriting { finished.signal() }
    guard finished.wait(timeout: .now() + 15) == .success, writer.status == .completed else {
        fatalError("Finish failed")
    }
}
print("Created four owned H.264 comparison fixtures")
