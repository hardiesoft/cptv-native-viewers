// Quick Look (spacebar) preview for .cptv: plays the clip in a loop, with the
// title and a frame counter underneath. Click the picture to pause/resume.
import Cocoa
import QuickLookUI

@objc(PreviewViewController)
final class PreviewViewController: NSViewController, QLPreviewingController {
    private let imageView = NSImageView()
    private let titleLabel = NSTextField(labelWithString: "")
    private let counterLabel = NSTextField(labelWithString: "")
    private let queue = DispatchQueue(label: "nz.org.cacophony.cptv-quicklook.render")
    private var clip: CPTVClip?        // only touched on `queue` after setup
    private var timer: DispatchSourceTimer?
    private var index = 0              // only touched on `queue`
    private var paused = false

    override func loadView() {
        let root = NSView(frame: NSRect(x: 0, y: 0, width: 640, height: 512))
        root.wantsLayer = true
        root.layer?.backgroundColor = NSColor.black.cgColor

        imageView.imageScaling = .scaleProportionallyUpOrDown
        for label in [titleLabel, counterLabel] {
            label.textColor = .lightGray
            label.font = .monospacedDigitSystemFont(ofSize: 12, weight: .regular)
            label.lineBreakMode = .byTruncatingTail
        }
        counterLabel.alignment = .right
        titleLabel.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)

        for v in [imageView, titleLabel, counterLabel] as [NSView] {
            v.translatesAutoresizingMaskIntoConstraints = false
            root.addSubview(v)
        }
        NSLayoutConstraint.activate([
            imageView.topAnchor.constraint(equalTo: root.topAnchor),
            imageView.leadingAnchor.constraint(equalTo: root.leadingAnchor),
            imageView.trailingAnchor.constraint(equalTo: root.trailingAnchor),
            titleLabel.topAnchor.constraint(equalTo: imageView.bottomAnchor, constant: 8),
            titleLabel.leadingAnchor.constraint(equalTo: root.leadingAnchor, constant: 10),
            titleLabel.bottomAnchor.constraint(equalTo: root.bottomAnchor, constant: -8),
            counterLabel.firstBaselineAnchor.constraint(equalTo: titleLabel.firstBaselineAnchor),
            counterLabel.leadingAnchor.constraint(greaterThanOrEqualTo: titleLabel.trailingAnchor, constant: 12),
            counterLabel.trailingAnchor.constraint(equalTo: root.trailingAnchor, constant: -10),
        ])
        root.addGestureRecognizer(NSClickGestureRecognizer(target: self, action: #selector(togglePause)))
        view = root
    }

    func preparePreviewOfFile(at url: URL, completionHandler handler: @escaping (Error?) -> Void) {
        queue.async { [weak self] in
            do {
                // Whole-clip normalisation keeps brightness steady while playing.
                let clip = try CPTVClip(url: url, normalizeClip: true, scale: 4)
                guard let first = clip.image(at: 0) else { throw CPTVError.noFrames }
                DispatchQueue.main.async {
                    guard let self else { return }
                    self.clip = clip
                    self.titleLabel.stringValue = clip.title ?? url.lastPathComponent
                    self.titleLabel.toolTip = clip.details
                    self.show(first, index: 0, of: clip)
                    self.preferredContentSize = NSSize(width: clip.width, height: clip.height + 32)
                    self.startTimer(fps: clip.fps)
                    handler(nil)
                }
            } catch {
                DispatchQueue.main.async { handler(error) }
            }
        }
    }

    private func startTimer(fps: Double) {
        let t = DispatchSource.makeTimerSource(queue: queue)
        let interval = 1.0 / fps
        t.schedule(deadline: .now() + interval, repeating: interval, leeway: .milliseconds(5))
        t.setEventHandler { [weak self] in self?.tick() }
        timer = t
        t.resume()
    }

    // Runs on `queue`.
    private func tick() {
        guard let clip else { return }
        index += 1
        var image = index < clip.frameCount ? clip.image(at: index) : nil
        if image == nil {          // end of clip (or header over-stated the count): loop
            index = 0
            image = clip.image(at: 0)
        }
        guard let image else { return }
        let i = index
        DispatchQueue.main.async { [weak self] in self?.show(image, index: i, of: clip) }
    }

    private func show(_ image: CGImage, index: Int, of clip: CPTVClip) {
        imageView.image = NSImage(cgImage: image, size: NSSize(width: image.width, height: image.height))
        let secs = Int(Double(index) / clip.fps)
        counterLabel.stringValue = String(format: "%d:%02d  %d/%d%@", secs / 60, secs % 60,
                                          index + 1, clip.frameCount, paused ? "  ⏸" : "")
    }

    @objc private func togglePause() {
        guard let timer else { return }
        paused.toggle()
        if paused { timer.suspend() } else { timer.resume() }
        if let current = imageView.image?.cgImage(forProposedRect: nil, context: nil, hints: nil),
           let clip {
            queue.async { [weak self] in
                let i = self?.index ?? 0
                DispatchQueue.main.async { self?.show(current, index: i, of: clip) }
            }
        }
    }

    deinit {
        if paused { timer?.resume() }   // a suspended source must be resumed before cancel
        timer?.cancel()
    }
}
