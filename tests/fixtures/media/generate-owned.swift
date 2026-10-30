// Procedural test-only frames. No camera, microphone, user media or UI access.
// Run: swift generate-owned.swift /absolute/owned-color.mp4
import AVFoundation
import CoreVideo
import Foundation

let url = URL(fileURLWithPath: CommandLine.arguments[1])
guard url.lastPathComponent == "owned-color.mp4",
      !FileManager.default.fileExists(atPath: url.path) else { fatalError("Use a new owned-color.mp4 test path") }
let width = 160, height = 96, frames = 60
let writer = try AVAssetWriter(outputURL: url, fileType: .mp4)
let input = AVAssetWriterInput(mediaType: .video, outputSettings: [
    AVVideoCodecKey: AVVideoCodecType.h264,
    AVVideoWidthKey: width,
    AVVideoHeightKey: height,
    AVVideoCompressionPropertiesKey: [AVVideoMaxKeyFrameIntervalKey: 20],
])
let adaptor = AVAssetWriterInputPixelBufferAdaptor(assetWriterInput: input, sourcePixelBufferAttributes: [
    kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_32ARGB,
    kCVPixelBufferWidthKey as String: width,
    kCVPixelBufferHeightKey as String: height,
])
input.expectsMediaDataInRealTime = false
writer.add(input)
guard writer.startWriting() else { fatalError("Writer failed") }
writer.startSession(atSourceTime: .zero)
for frame in 0..<frames {
    while !input.isReadyForMoreMediaData { Thread.sleep(forTimeInterval: 0.005) }
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
            base[offset + 1] = UInt8(50 + frame * 3)
            base[offset + 2] = UInt8((x * 255) / width)
            base[offset + 3] = UInt8((y * 255) / height)
        }
    }
    CVPixelBufferUnlockBaseAddress(pixel, [])
    guard adaptor.append(pixel, withPresentationTime: CMTime(value: Int64(frame), timescale: 20)) else { fatalError("Frame failed") }
}
input.markAsFinished()
let done = DispatchSemaphore(value: 0)
writer.finishWriting { done.signal() }
guard done.wait(timeout: .now() + 15) == .success, writer.status == .completed else { fatalError("Finish failed") }
print("Created owned three-second H.264 test video")
