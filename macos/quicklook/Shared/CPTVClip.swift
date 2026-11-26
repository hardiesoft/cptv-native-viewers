// Swift wrapper around the Rust CPTV core (vlc/cptv_core.h), shared by the
// Quick Look thumbnail and preview extensions. Not thread-safe: use one clip
// from one thread (or serial queue) at a time.
import CoreGraphics
import Foundation

enum CPTVError: LocalizedError {
    case open(String)
    case noFrames

    var errorDescription: String? {
        switch self {
        case .open(let msg): return "Couldn't open CPTV file: \(msg)"
        case .noFrames: return "The CPTV file has no frames."
        }
    }
}

final class CPTVClip {
    private let clip: OpaquePointer
    private let file: UnsafeMutablePointer<FILE>
    private let io: UnsafeMutablePointer<cptv_io>
    let info: cptv_info

    var width: Int { Int(info.width) }
    var height: Int { Int(info.height) }
    var frameCount: Int { Int(info.frame_count) }
    var fps: Double { info.fps > 0 ? info.fps : 9 }

    /// - normalizeClip: one brightness range for the whole clip (stable for
    ///   playback, but may read the whole file on open if the header lacks
    ///   min/max). Off stretches each frame on its own, which is cheap.
    init(url: URL, palette: String = "viridis", normalizeClip: Bool, scale: UInt32 = 4) throws {
        guard let f = fopen(url.path, "rb") else {
            throw CPTVError.open(String(cString: strerror(errno)))
        }
        file = f
        io = .allocate(capacity: 1)
        io.initialize(to: cptv_io(
            opaque: UnsafeMutableRawPointer(f),
            read: { opaque, buf, len in
                let fp = opaque!.assumingMemoryBound(to: FILE.self)
                let n = fread(buf, 1, len, fp)
                return (n == 0 && ferror(fp) != 0) ? -1 : n
            },
            seek: { opaque, offset in
                let fp = opaque!.assumingMemoryBound(to: FILE.self)
                return fseeko(fp, off_t(offset), SEEK_SET) == 0 ? 0 : -1
            }))

        var err = [CChar](repeating: 0, count: 256)
        guard let c = cptv_open_io(io, palette, normalizeClip ? 1 : 0, scale, &err, err.count) else {
            io.deallocate()
            fclose(f)
            throw CPTVError.open(String(cString: err))
        }
        clip = c
        var i = cptv_info()
        cptv_get_info(c, &i)
        info = i
    }

    deinit {
        cptv_close(clip)
        io.deallocate()
        fclose(file)
    }

    var title: String? { meta(Int32(CPTV_META_TITLE)) }
    var details: String? { meta(Int32(CPTV_META_DESCRIPTION)) }

    private func meta(_ key: Int32) -> String? {
        cptv_get_meta(clip, key).map { String(cString: $0) }
    }

    /// Renders frame `index`, or returns nil past the last frame that decodes.
    /// Going backwards rewinds and re-decodes from the start (streaming clips).
    func image(at index: Int) -> CGImage? {
        let pitch = width * 4
        var data = Data(count: pitch * height)
        let r = data.withUnsafeMutableBytes { buf in
            cptv_render(clip, UInt32(index), buf.baseAddress!.assumingMemoryBound(to: UInt8.self),
                        buf.count, pitch)
        }
        guard r == 1, let provider = CGDataProvider(data: data as CFData) else { return nil }
        // RV32 = native-endian 0xAARRGGBB, i.e. BGRA bytes on Apple silicon/Intel.
        let bitmap = CGBitmapInfo(rawValue: CGImageAlphaInfo.noneSkipFirst.rawValue
                                  | CGBitmapInfo.byteOrder32Little.rawValue)
        return CGImage(width: width, height: height, bitsPerComponent: 8, bitsPerPixel: 32,
                       bytesPerRow: pitch, space: CGColorSpace(name: CGColorSpace.sRGB)!,
                       bitmapInfo: bitmap, provider: provider, decode: nil,
                       shouldInterpolate: true, intent: .defaultIntent)
    }
}
