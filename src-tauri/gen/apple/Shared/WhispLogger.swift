#if os(iOS)
import Foundation

// FFI: Rust → Swift bridges so the Tauri command layer (running in the same
// process as the host app) can read the persistent log without needing to
// know the App Group container path. Strings returned by `whisp_log_read`
// must be freed via `whisp_log_free`.
@_cdecl("whisp_log_read")
public func whisp_log_read() -> UnsafeMutablePointer<CChar>? {
    WhispLogger.readCString()
}

@_cdecl("whisp_log_free")
public func whisp_log_free(_ ptr: UnsafeMutablePointer<CChar>?) {
    WhispLogger.freeCString(ptr)
}

@_cdecl("whisp_log_clear")
public func whisp_log_clear() {
    WhispLogger.clear()
}


// Persistent on-device log so users can surface diagnostics from Settings →
// Diagnostics without needing Console.app or a tethered Mac. Lives in the
// App Group container so both the host app and the Live Activity extension
// write to the same file.
//
// The host's Tauri command (read_ios_log) reads this and the React Settings
// screen renders it. NSLog is mirrored so we don't lose Console.app output.
public enum WhispLogger {
    private static let lock = NSLock()
    private static let maxBytes: Int = 256 * 1024
    private static let isoFormatter: ISO8601DateFormatter = {
        let f = ISO8601DateFormatter()
        f.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        return f
    }()

    public static var fileURL: URL? {
        FileManager.default
            .containerURL(forSecurityApplicationGroupIdentifier: whispAppGroupSuite)?
            .appendingPathComponent("whisp-ios.log")
    }

    public static func log(_ tag: String, _ message: String) {
        NSLog("[%@] %@", tag, message)
        let line = "\(isoFormatter.string(from: Date())) [\(tag)] \(message)\n"
        append(line)
    }

    public static func error(_ tag: String, _ message: String, _ error: Error? = nil) {
        let suffix: String
        if let e = error as NSError? {
            suffix = " | NSError(domain=\(e.domain), code=\(e.code), userInfo=\(e.userInfo))"
        } else {
            suffix = ""
        }
        log("\(tag) ERROR", message + suffix)
    }

    public static func read() -> String {
        guard let url = fileURL else { return "" }
        lock.lock(); defer { lock.unlock() }
        return (try? String(contentsOf: url, encoding: .utf8)) ?? ""
    }

    public static func clear() {
        guard let url = fileURL else { return }
        lock.lock(); defer { lock.unlock() }
        try? FileManager.default.removeItem(at: url)
    }

    // C-string allocation for FFI. Caller frees via `whisp_free_string`.
    fileprivate static func readCString() -> UnsafeMutablePointer<CChar>? {
        let s = read()
        return s.withCString { src -> UnsafeMutablePointer<CChar>? in
            let len = strlen(src) + 1
            let dst = UnsafeMutablePointer<CChar>.allocate(capacity: len)
            dst.initialize(from: src, count: len)
            return dst
        }
    }

    fileprivate static func freeCString(_ ptr: UnsafeMutablePointer<CChar>?) {
        ptr?.deallocate()
    }

    private static func append(_ line: String) {
        guard let url = fileURL, let data = line.data(using: .utf8) else { return }
        lock.lock(); defer { lock.unlock() }
        let fm = FileManager.default
        if !fm.fileExists(atPath: url.path) {
            try? data.write(to: url, options: .atomic)
            return
        }
        // Rotate by truncate-and-replace once we exceed maxBytes. Keep the
        // tail half so context around a recent failure survives.
        if let attrs = try? fm.attributesOfItem(atPath: url.path),
           let size = attrs[.size] as? Int, size > maxBytes {
            if let handle = try? FileHandle(forReadingFrom: url) {
                defer { try? handle.close() }
                let offset = UInt64(size - maxBytes / 2)
                if (try? handle.seek(toOffset: offset)) != nil,
                   let tail = try? handle.readToEnd() {
                    var truncated = Data("--- log rotated ---\n".utf8)
                    truncated.append(tail)
                    truncated.append(data)
                    try? truncated.write(to: url, options: .atomic)
                    return
                }
            }
        }
        if let handle = try? FileHandle(forWritingTo: url) {
            defer { try? handle.close() }
            _ = try? handle.seekToEnd()
            try? handle.write(contentsOf: data)
        }
    }
}
#endif
