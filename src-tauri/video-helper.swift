import AVFoundation
import AppKit
import CoreImage
import CoreVideo
import Foundation

@main
struct SqueezerVideoHelper {
    static func main() async {
        guard CommandLine.arguments.count >= 3 else {
            fputs("Usage: squeezer-video-helper analyze FILE | encode INPUT OUTPUT BITRATE MAX_DIMENSION\n", stderr)
            exit(2)
        }
        if CommandLine.arguments[1] == "encode" {
            guard CommandLine.arguments.count == 6,
                  let bitrate = Int(CommandLine.arguments[4]), let maxDimension = Int(CommandLine.arguments[5]) else {
                fputs("encode requires input, output, bitrate and max dimension\n", stderr)
                exit(2)
            }
            do { try await encode(input: URL(fileURLWithPath: CommandLine.arguments[2]), output: URL(fileURLWithPath: CommandLine.arguments[3]), bitrate: bitrate, maxDimension: maxDimension) }
            catch { fputs("\(error.localizedDescription)\n", stderr); exit(1) }
            return
        }
        guard CommandLine.arguments[1] == "analyze", CommandLine.arguments.count == 3 else {
            fputs("Usage: squeezer-video-helper analyze FILE | encode INPUT OUTPUT BITRATE MAX_DIMENSION\n", stderr)
            exit(2)
        }
        let url = URL(fileURLWithPath: CommandLine.arguments[2])
        do {
            let asset = AVURLAsset(url: url)
            let duration = CMTimeGetSeconds(try await asset.load(.duration))
            guard duration.isFinite, duration > 0 else { throw HelperError.invalid("影片時長無效") }
            let videos = try await asset.loadTracks(withMediaType: .video)
            guard videos.count == 1, let video = videos.first else { throw HelperError.invalid("影片必須包含一條視訊軌") }
            let audios = try await asset.loadTracks(withMediaType: .audio)
            guard audios.count <= 1 else { throw HelperError.invalid("目前不支援多音訊軌影片") }
            let allTracks = try await asset.load(.tracks)
            if allTracks.contains(where: { [.text, .subtitle, .closedCaption].contains($0.mediaType) }) {
                throw HelperError.invalid("目前不支援含字幕或章節軌的影片")
            }
            let size = try await video.load(.naturalSize)
            let transform = try await video.load(.preferredTransform)
            let width = Int(abs(size.applying(transform).width).rounded())
            let height = Int(abs(size.applying(transform).height).rounded())
            let videoCodec = try await codecName(video)
            if try await containsHdr(video) { throw HelperError.invalid("目前不支援 HDR 或 Dolby Vision 影片") }
            let audioCodec: String?
            if let audioTrack = audios.first { audioCodec = try await codecName(audioTrack) } else { audioCodec = nil }
            guard ["avc1", "hvc1", "hev1"].contains(videoCodec) else { throw HelperError.invalid("目前只支援 H.264 與 HEVC 影片") }
            if let audioCodec, !["mp4a", "aac"].contains(audioCodec) { throw HelperError.invalid("目前只支援 AAC 音訊") }

            let generator = AVAssetImageGenerator(asset: asset)
            generator.appliesPreferredTrackTransform = true
            generator.maximumSize = CGSize(width: 480, height: 360)
            let frame = try generator.copyCGImage(at: CMTime(seconds: min(1, duration / 4), preferredTimescale: 600), actualTime: nil)
            let image = NSBitmapImageRep(cgImage: frame)
            guard let jpeg = image.representation(using: .jpeg, properties: [.compressionFactor: 0.78]) else { throw HelperError.invalid("無法產生影片縮圖") }
            let fileAttributes = try FileManager.default.attributesOfItem(atPath: url.path)
            let result: [String: Any] = [
                "fileName": url.lastPathComponent,
                "extension": url.pathExtension.lowercased(),
                "width": width,
                "height": height,
                "durationMs": Int64(duration * 1000),
                "codec": videoCodec,
                "audioCodec": audioCodec as Any? ?? NSNull(),
                "thumbnail": "data:image/jpeg;base64,\(jpeg.base64EncodedString())",
                "originalBytes": (fileAttributes[.size] as? NSNumber)?.uint64Value ?? 0,
            ]
            let output = try JSONSerialization.data(withJSONObject: result, options: [.sortedKeys])
            FileHandle.standardOutput.write(output)
            FileHandle.standardOutput.write(Data([0x0A]))
        } catch {
            fputs("\(error.localizedDescription)\n", stderr)
            exit(1)
        }
    }

    static func codecName(_ track: AVAssetTrack) async throws -> String {
        let descriptions = try await track.load(.formatDescriptions)
        guard let description = descriptions.first else { throw HelperError.invalid("無法讀取媒體編碼資訊") }
        let code = CMFormatDescriptionGetMediaSubType(description)
        let bytes = [UInt8((code >> 24) & 0xff), UInt8((code >> 16) & 0xff), UInt8((code >> 8) & 0xff), UInt8(code & 0xff)]
        return String(bytes: bytes, encoding: .ascii)?.trimmingCharacters(in: .whitespacesAndNewlines) ?? "unknown"
    }

    static func containsHdr(_ track: AVAssetTrack) async throws -> Bool {
        let descriptions = try await track.load(.formatDescriptions)
        guard let description = descriptions.first else { return false }
        guard let extensions = CMFormatDescriptionGetExtensions(description) else { return false }
        let values = (extensions as NSDictionary).allValues.map(String.init(describing:)).joined(separator: " ").lowercased()
        return values.contains("2084") || values.contains("2100") || values.contains("hlg") || values.contains("dolby")
    }

    static func encode(input inputURL: URL, output outputURL: URL, bitrate: Int, maxDimension: Int) async throws {
        let asset = AVURLAsset(url: inputURL)
        let duration = CMTimeGetSeconds(try await asset.load(.duration))
        let videos = try await asset.loadTracks(withMediaType: .video)
        guard let video = videos.first else { throw HelperError.invalid("影片缺少視訊軌") }
        let audios = try await asset.loadTracks(withMediaType: .audio)
        let naturalSize = try await video.load(.naturalSize)
        let frameRate = try await video.load(.nominalFrameRate)
        let preferredTransform = try await video.load(.preferredTransform)
        let transformed = CGRect(origin: .zero, size: naturalSize).applying(preferredTransform).standardized
        let scale = maxDimension > 0 ? min(1, Double(maxDimension) / Double(max(transformed.width, transformed.height))) : 1
        let outputWidth = max(2, Int((transformed.width * scale).rounded()) / 2 * 2)
        let outputHeight = max(2, Int((transformed.height * scale).rounded()) / 2 * 2)

        let reader = try AVAssetReader(asset: asset)
        let composition = AVMutableVideoComposition()
        composition.renderSize = CGSize(width: outputWidth, height: outputHeight)
        composition.frameDuration = CMTime(seconds: 1 / Double(max(frameRate, 1)), preferredTimescale: 60_000)
        let instruction = AVMutableVideoCompositionInstruction()
        instruction.timeRange = CMTimeRange(start: .zero, duration: try await asset.load(.duration))
        let layer = AVMutableVideoCompositionLayerInstruction(assetTrack: video)
        let translate = CGAffineTransform(translationX: -transformed.minX, y: -transformed.minY)
        layer.setTransform(preferredTransform.concatenating(translate).scaledBy(x: scale, y: scale), at: .zero)
        instruction.layerInstructions = [layer]
        composition.instructions = [instruction]
        let videoOutput = AVAssetReaderVideoCompositionOutput(videoTracks: [video], videoSettings: [kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_32BGRA])
        videoOutput.videoComposition = composition
        videoOutput.alwaysCopiesSampleData = false
        guard reader.canAdd(videoOutput) else { throw HelperError.invalid("無法讀取影片影格") }
        reader.add(videoOutput)
        let audioOutput: AVAssetReaderTrackOutput?
        let audioInput: AVAssetWriterInput?
        if let audio = audios.first {
            let output = AVAssetReaderTrackOutput(track: audio, outputSettings: nil)
            output.alwaysCopiesSampleData = false
            guard reader.canAdd(output) else { throw HelperError.invalid("無法保留 AAC 音訊") }
            reader.add(output)
            let descriptions = try await audio.load(.formatDescriptions)
            let input = AVAssetWriterInput(mediaType: .audio, outputSettings: nil, sourceFormatHint: descriptions.first)
            audioOutput = output
            audioInput = input
        } else { audioOutput = nil; audioInput = nil }

        let writer = try AVAssetWriter(outputURL: outputURL, fileType: .mp4)
        let videoInput = AVAssetWriterInput(mediaType: .video, outputSettings: [
            AVVideoCodecKey: AVVideoCodecType.h264,
            AVVideoWidthKey: outputWidth,
            AVVideoHeightKey: outputHeight,
            AVVideoCompressionPropertiesKey: [AVVideoAverageBitRateKey: bitrate, AVVideoMaxKeyFrameIntervalKey: 60],
        ])
        videoInput.expectsMediaDataInRealTime = false
        guard writer.canAdd(videoInput) else { throw HelperError.invalid("無法建立 H.264 輸出") }
        writer.add(videoInput)
        if let audioInput { guard writer.canAdd(audioInput) else { throw HelperError.invalid("無法加入音訊輸出") }; writer.add(audioInput) }
        guard reader.startReading(), writer.startWriting() else { throw reader.error ?? writer.error ?? HelperError.invalid("無法開始影片編碼") }
        writer.startSession(atSourceTime: .zero)
        let group = DispatchGroup()
        let progressLock = NSLock()
        var callbackError: Error?
        var maxProgress = 0.0
        var lastProgress = Date.distantPast
        let videoQueue = DispatchQueue(label: "com.squeezer.video-writer.video")
        group.enter()
        videoInput.requestMediaDataWhenReady(on: videoQueue) {
            while videoInput.isReadyForMoreMediaData {
                guard let sample = videoOutput.copyNextSampleBuffer() else { videoInput.markAsFinished(); group.leave(); return }
                guard videoInput.append(sample) else {
                    progressLock.lock(); callbackError = writer.error ?? HelperError.invalid("影片影格寫入失敗"); progressLock.unlock()
                    reader.cancelReading(); group.leave(); return
                }
                let time = CMTimeGetSeconds(CMSampleBufferGetPresentationTimeStamp(sample))
                progressLock.lock()
                maxProgress = max(maxProgress, time)
                if Date().timeIntervalSince(lastProgress) >= 0.5 {
                    print(String(format: "progress=%.4f", min(1, maxProgress / duration)))
                    fflush(stdout)
                    lastProgress = Date()
                }
                progressLock.unlock()
            }
        }
        if let audioInput, let audioOutput {
            let audioQueue = DispatchQueue(label: "com.squeezer.video-writer.audio")
            group.enter()
            audioInput.requestMediaDataWhenReady(on: audioQueue) {
                while audioInput.isReadyForMoreMediaData {
                    guard let sample = audioOutput.copyNextSampleBuffer() else { audioInput.markAsFinished(); group.leave(); return }
                    guard audioInput.append(sample) else {
                        progressLock.lock(); callbackError = writer.error ?? HelperError.invalid("音訊寫入失敗"); progressLock.unlock()
                        reader.cancelReading(); group.leave(); return
                    }
                }
            }
        }
        group.wait()
        progressLock.lock()
        let writeError = callbackError
        progressLock.unlock()
        if let writeError { throw writeError }
        guard reader.status == .completed || reader.status == .reading else { throw reader.error ?? HelperError.invalid("影片來源讀取失敗") }
        if writer.status == .failed { throw writer.error ?? HelperError.invalid("影片編碼失敗") }
        let semaphore = DispatchSemaphore(value: 0)
        writer.finishWriting { semaphore.signal() }
        semaphore.wait()
        guard writer.status == .completed else { throw writer.error ?? HelperError.invalid("影片輸出未完成") }
        print("progress=1.0000")
    }
}

enum HelperError: LocalizedError {
    case invalid(String)
    var errorDescription: String? { if case let .invalid(message) = self { return message }; return nil }
}
