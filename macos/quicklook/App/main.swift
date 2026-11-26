// Host app for the CPTV Quick Look extensions. It only exists to carry them
// and to declare the .cptv file type; opening it just says so.
import AppKit

let app = NSApplication.shared
app.setActivationPolicy(.regular)
app.activate(ignoringOtherApps: true)
let alert = NSAlert()
alert.messageText = "CPTV Quick Look is installed"
alert.informativeText = """
    Finder now shows thumbnails for .cptv thermal video files, and pressing \
    Space on one plays a preview. There's nothing else to do here.
    """
alert.runModal()
