import AVFoundation
import Darwin
import Foundation

func option(_ name: String) -> String? {
    guard let index = CommandLine.arguments.firstIndex(of: name), CommandLine.arguments.indices.contains(index + 1) else { return nil }
    return CommandLine.arguments[index + 1]
}

func showUsage() {
    print("Usage: swift avfoundation-reader-writer.swift --input FILE --output FILE [--codec h264|hevc] [--bitrate BPS | --target-bytes BYTES]")
    print("       swift avfoundation-reader-writer.swift --help")
}

func fail(_ message: String) -> Never {
    fputs("\(message)\n", stderr)
    exit(EXIT_FAILURE)
}

func finishWritingAndWait(_ writer: AVAssetWriter) {
    let semaphore = DispatchSemaphore(value: 0)
    writer.finishWriting { semaphore.signal() }
    semaphore.wait()
}

if CommandLine.arguments.contains("--help") || CommandLine.arguments.count == 1 {
    showUsage()
} else {
    guard let inputPath = option("--input"), let outputPath = option("--output") else {
        showUsage()
        exit(EXIT_FAILURE)
    }

    let inputURL = URL(fileURLWithPath: inputPath)
    let outputURL = URL(fileURLWithPath: outputPath)
    guard FileManager.default.fileExists(atPath: inputURL.path) else { fail("Input file does not exist") }
    guard !FileManager.default.fileExists(atPath: outputURL.path) else { fail("Output already exists") }
    let codecName = option("--codec") ?? "h264"
    let codec: AVVideoCodecType
    switch codecName {
    case "h264": codec = .h264
    case "hevc": codec = .hevc
    default: fail("--codec must be h264 or hevc")
    }
    let targetBytes = option("--target-bytes").flatMap(Int64.init)
    let explicitBitrate = option("--bitrate").flatMap(Int.init)
    if targetBytes != nil && explicitBitrate != nil { fail("Choose either --target-bytes or --bitrate") }

    do {
        let asset = AVURLAsset(url: inputURL)
        let duration = CMTimeGetSeconds(try await asset.load(.duration))
        guard duration.isFinite, duration > 0 else { fail("Input duration is invalid") }
        let videoTracks = try await asset.loadTracks(withMediaType: .video)
        guard let videoTrack = videoTracks.first else { fail("Input has no video track") }
        let audioTracks = try await asset.loadTracks(withMediaType: .audio)
        let size = try await videoTrack.load(.naturalSize)
        let transform = try await videoTrack.load(.preferredTransform)
        let width = max(2, Int(abs(size.width).rounded()))
        let height = max(2, Int(abs(size.height).rounded()))
        let audioBitrateReserve = audioTracks.isEmpty ? 0 : 128_000
        let bitrate: Int
        if let targetBytes {
            bitrate = max(100_000, Int(Double(targetBytes * 8) / duration) - audioBitrateReserve)
        } else {
            bitrate = explicitBitrate ?? max(300_000, width * height * 3)
        }

        let reader = try AVAssetReader(asset: asset)
        let videoOutput = AVAssetReaderTrackOutput(track: videoTrack, outputSettings: [
            kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_32BGRA,
        ])
        videoOutput.alwaysCopiesSampleData = false
        guard reader.canAdd(videoOutput) else { fail("Cannot add video reader output") }
        reader.add(videoOutput)

        let audioOutput: AVAssetReaderTrackOutput?
        let audioInput: AVAssetWriterInput?
        if let audioTrack = audioTracks.first {
            let output = AVAssetReaderTrackOutput(track: audioTrack, outputSettings: nil)
            output.alwaysCopiesSampleData = false
            guard reader.canAdd(output) else { fail("Cannot add audio reader output") }
            reader.add(output)
            audioOutput = output
            let formatDescriptions = try await audioTrack.load(.formatDescriptions)
            guard let formatHint = formatDescriptions.first else { fail("Cannot determine source audio format") }
            let input = AVAssetWriterInput(mediaType: .audio, outputSettings: nil, sourceFormatHint: formatHint)
            audioInput = input
        } else {
            audioOutput = nil
            audioInput = nil
        }

        let writer = try AVAssetWriter(outputURL: outputURL, fileType: .mp4)
        let videoSettings: [String: Any] = [
            AVVideoCodecKey: codec,
            AVVideoWidthKey: width,
            AVVideoHeightKey: height,
            AVVideoCompressionPropertiesKey: [AVVideoAverageBitRateKey: bitrate],
        ]
        let videoInput = AVAssetWriterInput(mediaType: .video, outputSettings: videoSettings)
        videoInput.transform = transform
        guard writer.canAdd(videoInput) else { fail("Cannot add video writer input") }
        writer.add(videoInput)
        if let audioInput {
            guard writer.canAdd(audioInput) else { fail("Cannot add audio writer input") }
            writer.add(audioInput)
        }

        guard reader.startReading() else { throw reader.error ?? NSError(domain: "VideoProbe", code: 1) }
        guard writer.startWriting() else { throw writer.error ?? NSError(domain: "VideoProbe", code: 2) }
        writer.startSession(atSourceTime: .zero)
        var nextVideo = videoOutput.copyNextSampleBuffer()
        var nextAudio = audioOutput?.copyNextSampleBuffer()
        let started = Date()
        var lastProgress = Date.distantPast

        while nextVideo != nil || nextAudio != nil {
            if let cancelAfter = option("--cancel-after").flatMap(Double.init), Date().timeIntervalSince(started) >= cancelAfter {
                reader.cancelReading()
                writer.cancelWriting()
                try? FileManager.default.removeItem(at: outputURL)
                fail("Cancelled after \(cancelAfter) seconds")
            }

            let videoTime = nextVideo.map(CMSampleBufferGetPresentationTimeStamp)
            let audioTime = nextAudio.map(CMSampleBufferGetPresentationTimeStamp)
            let chooseVideo = nextAudio == nil || (nextVideo != nil && CMTimeCompare(videoTime!, audioTime!) <= 0)
            let progressTime = chooseVideo ? (videoTime ?? .zero) : (audioTime ?? .zero)
            if chooseVideo {
                if videoInput.isReadyForMoreMediaData, let sample = nextVideo {
                    guard videoInput.append(sample) else { throw writer.error ?? NSError(domain: "VideoProbe", code: 3) }
                    nextVideo = videoOutput.copyNextSampleBuffer()
                } else {
                    usleep(2_000)
                }
            } else if let audioInput, audioInput.isReadyForMoreMediaData, let sample = nextAudio {
                guard audioInput.append(sample) else { throw writer.error ?? NSError(domain: "VideoProbe", code: 4) }
                nextAudio = audioOutput?.copyNextSampleBuffer()
            } else {
                usleep(2_000)
            }

            if Date().timeIntervalSince(lastProgress) >= 1 {
                let currentTime = CMTimeGetSeconds(progressTime)
                print(String(format: "progress=%.3f", min(1, currentTime / duration)))
                lastProgress = Date()
            }
        }

        videoInput.markAsFinished()
        audioInput?.markAsFinished()
        finishWritingAndWait(writer)
        guard writer.status == .completed else { throw writer.error ?? NSError(domain: "VideoProbe", code: 5) }

        let outputBytes = try FileManager.default.attributesOfItem(atPath: outputURL.path)[.size] as? NSNumber
        let outputAsset = AVURLAsset(url: outputURL)
        let outputDuration = CMTimeGetSeconds(try await outputAsset.load(.duration))
        let outputVideos = try await outputAsset.loadTracks(withMediaType: .video)
        let outputAudios = try await outputAsset.loadTracks(withMediaType: .audio)
        print("codec=\(codecName)")
        print("requested_video_bitrate=\(bitrate)")
        print("requested_target_bytes=\(targetBytes.map(String.init) ?? "none")")
        print("input_duration_seconds=\(duration)")
        print("output_duration_seconds=\(outputDuration)")
        print("output_video_tracks=\(outputVideos.count)")
        print("output_audio_tracks=\(outputAudios.count)")
        print("output_bytes=\(outputBytes?.int64Value ?? -1)")
        print("elapsed_seconds=\(Date().timeIntervalSince(started))")
    } catch {
        try? FileManager.default.removeItem(at: outputURL)
        fputs("Encode failed: \(error)\n", stderr)
        exit(EXIT_FAILURE)
    }
}
