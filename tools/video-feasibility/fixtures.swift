import Foundation
import Darwin

struct Fixture {
    let name: String
    let size: String
    let duration: Int
    let encoder: String
    let quality: String
    let audio: Bool
    let portrait: Bool
}

struct FixtureGenerator {
    static let fixtures = [
        Fixture(name: "short-h264-silent", size: "1280x720", duration: 8, encoder: "libx264", quality: "20", audio: false, portrait: false),
        Fixture(name: "short-h264-aac-motion", size: "1920x1080", duration: 12, encoder: "libx264", quality: "22", audio: true, portrait: false),
        Fixture(name: "portrait-h264-aac", size: "1080x1920", duration: 12, encoder: "libx264", quality: "22", audio: true, portrait: true),
        Fixture(name: "long-hevc-aac", size: "1280x720", duration: 90, encoder: "libx265", quality: "26", audio: true, portrait: false),
        Fixture(name: "short-4k-hevc-silent", size: "3840x2160", duration: 6, encoder: "libx265", quality: "28", audio: false, portrait: false),
    ]

    static func main() throws {
        let arguments = Array(CommandLine.arguments.dropFirst())
        if arguments.contains("--help") || arguments.isEmpty {
            print("""
            Usage: swift fixtures.swift --output-dir DIRECTORY [--ffmpeg PATH]
            Generates deterministic synthetic MP4 inputs for encoder comparisons.
            FFmpeg is used only to generate fixtures; its output is not a candidate benchmark result.
            """)
            return
        }

        guard let outputArgument = value(after: "--output-dir", in: arguments) else {
            throw FixtureError.usage("Missing --output-dir")
        }
        let outputDirectory = URL(fileURLWithPath: outputArgument, isDirectory: true)
        let ffmpeg = try value(after: "--ffmpeg", in: arguments).map(URL.init(fileURLWithPath:)) ?? resolveFFmpeg()
        try FileManager.default.createDirectory(at: outputDirectory, withIntermediateDirectories: true)

        for fixture in fixtures {
            let output = outputDirectory.appendingPathComponent("\(fixture.name).mp4")
            var processArguments = [
                "-hide_banner", "-loglevel", "error", "-y",
                "-f", "lavfi", "-i", "testsrc2=size=\(fixture.size):rate=30",
            ]
            if fixture.audio {
                processArguments += ["-f", "lavfi", "-i", "sine=frequency=997:sample_rate=48000"]
            }
            processArguments += ["-t", String(fixture.duration), "-map", "0:v:0"]
            if fixture.audio { processArguments += ["-map", "1:a:0"] }
            processArguments += [
                "-c:v", fixture.encoder,
                "-preset", "ultrafast",
                "-crf", fixture.quality,
                "-pix_fmt", "yuv420p",
                "-threads", "1",
            ]
            if fixture.encoder == "libx265" { processArguments += ["-tag:v", "hvc1"] }
            if fixture.audio { processArguments += ["-c:a", "aac", "-b:a", "128k"] }
            processArguments += ["-movflags", "+faststart", output.path]
            try run(ffmpeg, arguments: processArguments)
            print("\(fixture.name)\t\(output.path)")
        }
    }

    private static func value(after option: String, in arguments: [String]) -> String? {
        guard let index = arguments.firstIndex(of: option), arguments.indices.contains(index + 1) else { return nil }
        return arguments[index + 1]
    }

    private static func resolveFFmpeg() throws -> URL {
        let paths = (ProcessInfo.processInfo.environment["PATH"] ?? "").split(separator: ":")
        if let path = paths.map({ URL(fileURLWithPath: String($0)).appendingPathComponent("ffmpeg") }).first(where: { FileManager.default.isExecutableFile(atPath: $0.path) }) {
            return path
        }
        throw FixtureError.usage("FFmpeg was not found in PATH; pass --ffmpeg PATH")
    }

    private static func run(_ executable: URL, arguments: [String]) throws {
        let process = Process()
        let errors = Pipe()
        process.executableURL = executable
        process.arguments = arguments
        process.standardError = errors
        try process.run()
        process.waitUntilExit()
        let errorOutput = String(decoding: errors.fileHandleForReading.readDataToEndOfFile(), as: UTF8.self)
        guard process.terminationStatus == 0 else {
            throw FixtureError.generation(executable.lastPathComponent, process.terminationStatus, errorOutput)
        }
    }
}

enum FixtureError: Error, CustomStringConvertible {
    case usage(String)
    case generation(String, Int32, String)

    var description: String {
        switch self {
        case .usage(let message): return message
        case .generation(let executable, let status, let detail): return "\(executable) exited with \(status): \(detail)"
        }
    }
}

do {
    try FixtureGenerator.main()
} catch {
    fputs("fixture generation failed: \(error)\n", stderr)
    exit(EXIT_FAILURE)
}
