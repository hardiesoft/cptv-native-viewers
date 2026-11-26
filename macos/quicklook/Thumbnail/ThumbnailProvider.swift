// Finder thumbnails for .cptv: one frame about a second into the clip.
import CoreGraphics
import QuickLookThumbnailing

@objc(ThumbnailProvider)
final class ThumbnailProvider: QLThumbnailProvider {
    override func provideThumbnail(for request: QLFileThumbnailRequest,
                                   _ handler: @escaping (QLThumbnailReply?, Error?) -> Void) {
        do {
            // Sensor frames are 160 px wide; upscale only as far as the request needs.
            let pixels = max(request.maximumSize.width, request.maximumSize.height) * request.scale
            let scale = UInt32(min(4, max(1, (pixels / 160).rounded(.up))))
            // Per-frame normalisation never needs a pass over the whole file, and
            // streamOnly never decodes the whole clip into memory (older files have
            // no frame count in the header; a 10-minute one would take ~200 MB).
            let clip = try CPTVClip(url: request.fileURL, normalizeClip: false, scale: scale,
                                    streamOnly: true)

            // Frames are delta-coded in one gzip stream, so frame N costs N decodes.
            // About one second in skips any start-up frames but stays cheap; a
            // shorter clip gives its last frame (the frame count may be unknown).
            guard let image = clip.image(at: Int(clip.fps.rounded()), clamped: true) else {
                throw CPTVError.noFrames
            }

            let aspect = CGFloat(image.width) / CGFloat(image.height)
            let box = request.maximumSize
            let size = box.width / box.height > aspect
                ? CGSize(width: (box.height * aspect).rounded(), height: box.height)
                : CGSize(width: box.width, height: (box.width / aspect).rounded())

            let reply = QLThumbnailReply(contextSize: size) { ctx in
                // The context is contextSize × request.scale pixels but isn't
                // pre-scaled to points, so fill whatever its full extent is in
                // user space (drawing at `size` filled 1/4 of it on Retina).
                let full = ctx.convertToUserSpace(CGRect(x: 0, y: 0, width: ctx.width, height: ctx.height))
                ctx.interpolationQuality = .high
                ctx.draw(image, in: full)
                return true
            }
            // Without this Finder wraps the picture in a document icon with a
            // folded corner. `iconFlavor` is private API using the old QL
            // generator flavours; 1 ("shadow") gives rounded corners and a drop
            // shadow, like photo thumbnails. Only set it if it's still there.
            if reply.responds(to: NSSelectorFromString("setIconFlavor:")) {
                reply.setValue(1, forKey: "iconFlavor")
            }
            handler(reply, nil)
        } catch {
            handler(nil, error)
        }
    }
}
