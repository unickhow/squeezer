import AVFoundation
import Darwin
import Foundation

func value(after option: String) -> String? {
    guard let index = CommandLine.arguments.firstIndex(of: option), CommandLine.arguments.indices.contains(index + 1) else { return nil }
    return CommandLine.arguments[index + 1]
}

func usage() {
    print("Usage: swift avfoundation-export.swift --input FILE --output FILE [--preset PRESET] [--file-length-limit BYTES]")
    print("       swift avfoundation-export.swift --list-presets")
}

if CommandLine.arguments.contains("--help") || CommandLine.arguments.count == 1 {
    usage()
} else if CommandLine.arguments.contains("--list-presets") {
    for preset in AVAssetExportSession.allExportPresets() { print(preset) }
} else {
    guard let inputPath = value(after: "--input"), let outputPath = value(after: "--output") else {
        usage()
        exit(EXIT_FAILURE)
    }

    let input = URL(fileURLWithPath: inputPath)
    let output = URL(fileURLWithPath: outputPath)
    guard FileManager.default.fileExists(atPath: input.path) else {
        fputs("Input file does not exist: \(input.path)\n", stderr)
        exit(EXIT_FAILURE)
    }
    guard !FileManager.default.fileExists(atPath: output.path) else {
        fputs("Output already exists: \(output.path)\n", stderr)
        exit(EXIT_FAILURE)
    }

    let preset = value(after: "--preset") ?? AVAssetExportPresetHighestQuality
    let asset = AVURLAsset(url: input)
    guard let session = AVAssetExportSession(asset: asset, presetName: preset) else {
        fputs("Preset is unavailable for this asset: \(preset)\n", stderr)
        exit(EXIT_FAILURE)
    }
    guard session.supportedFileTypes.contains(.mp4) else {
        fputs("Preset does not support MP4 output: \(preset)\n", stderr)
        exit(EXIT_FAILURE)
    }
    if let limit = value(after: "--file-length-limit").flatMap(Int64.init) {
        session.fileLengthLimit = limit
    }

    let start = ContinuousClock.now
    do {
        try await session.export(to: output, as: .mp4)
        let elapsed = start.duration(to: .now)
        let fileBytes = try FileManager.default.attributesOfItem(atPath: output.path)[.size] as? NSNumber
        let duration = try await asset.load(.duration)
        let videoTracks = try await asset.loadTracks(withMediaType: .video)
        let audioTracks = try await asset.loadTracks(withMediaType: .audio)
        print("preset=\(preset)")
        print("supported_file_types=\(session.supportedFileTypes.map(\.rawValue).joined(separator: ","))")
        print("input_duration_seconds=\(CMTimeGetSeconds(duration))")
        print("input_video_tracks=\(videoTracks.count)")
        print("input_audio_tracks=\(audioTracks.count)")
        print("output_bytes=\(fileBytes?.int64Value ?? -1)")
        print("elapsed_seconds=\(Double(elapsed.components.seconds) + Double(elapsed.components.attoseconds) / 1e18)")
    } catch {
        try? FileManager.default.removeItem(at: output)
        fputs("Export failed: \(error)\n", stderr)
        exit(EXIT_FAILURE)
    }
}
