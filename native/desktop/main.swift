import AppKit
import ApplicationServices

let args = CommandLine.arguments
if args.count == 2 && args[1] == "toggle" {
    let current = NSWorkspace.shared.frontmostApplication?.bundleIdentifier
    let target = current == claudeBundle ? codexBundle : claudeBundle
    guard let url = NSWorkspace.shared.urlForApplication(withBundleIdentifier: target) else { fail("Desktop app unavailable") }
    exit(NSWorkspace.shared.open(url) ? 0 : 1)
}
guard args.count == 4, args[1] == "voice", let url = URL(string: args[2]),
      let id = identity(url), !id.isEmpty else { fail("Invalid session destination") }
let bundle = url.scheme == "codex" ? codexBundle : claudeBundle
// Opening the selected session remains useful if Accessibility has not been enabled.
guard NSWorkspace.shared.open(url) else { fail("Could not open the selected session") }
let trustOptions = [kAXTrustedCheckOptionPrompt.takeUnretainedValue() as String: true] as CFDictionary
guard AXIsProcessTrustedWithOptions(trustOptions) else { fail("Enable Accessibility for Token Café to start native voice") }
let deadline = Date().addingTimeInterval(8)
var enabledAccessibilityFor: pid_t?
var phase = "waiting-for-app"
var nodeCount = 0
var buttonCount = 0
var labelledCount = 0
var disabledCount = 0
while Date() < deadline {
    guard let app = NSWorkspace.shared.frontmostApplication, app.bundleIdentifier == bundle else {
        pauseForAppUpdate(); continue
    }
    let root = AXUIElementCreateApplication(app.processIdentifier)
    AXUIElementSetMessagingTimeout(root, 0.5)
    if enabledAccessibilityFor != app.processIdentifier {
        // Electron explicitly requires this for external assistive controls.
        // This exposes its existing controls; macOS Accessibility authorization
        // is still checked above and is never changed here.
        let result = AXUIElementSetAttributeValue(root, "AXManualAccessibility" as CFString, kCFBooleanTrue)
        fputs("Keypad voice: accessibility=\(result.rawValue) provider=\(bundle == codexBundle ? "codex" : "claude")\n", stderr)
        enabledAccessibilityFor = app.processIdentifier
        pauseForAppUpdate()
    }
    phase = "waiting-for-window"
    guard let rawWindow = attribute(root, kAXFocusedWindowAttribute), CFGetTypeID(rawWindow) == AXUIElementGetTypeID() else {
        pauseForAppUpdate(); continue
    }
    let window = unsafeBitCast(rawWindow, to: AXUIElement.self)
    let nodes = walk(window)
    nodeCount = nodes.count
    phase = "waiting-for-session"
    if sessionMatches(nodes, id: id, title: args[3]) {
        let labels = recordingLabels(for: bundle)
        let labelled = nodes.filter { node in
            ["AXButton", "AXCheckBox", "AXStaticText"].contains(string(node, kAXRoleAttribute)) &&
            voiceControlMatches(node, labels: labels)
        }
        labelledCount = labelled.count
        let controls = labelled.filter { node in
            ["AXButton", "AXCheckBox"].contains(string(node, kAXRoleAttribute))
        }
        let buttons = controls.filter { (attribute($0, kAXEnabledAttribute) as? Bool) != false }
        disabledCount = controls.count - buttons.count
        buttonCount = buttons.count
        phase = "waiting-for-voice-control"
        if buttons.count == 1 {
            // Recheck focus just before the action: switching apps cancels activation.
            guard NSWorkspace.shared.frontmostApplication?.processIdentifier == app.processIdentifier else { fail("Focus changed") }
            guard AXUIElementPerformAction(buttons[0], kAXPressAction as CFString) == .success else { fail("Native voice control rejected activation") }
            exit(0)
        }
    }
    pauseForAppUpdate()
}
fail("Native voice unavailable: \(phase), nodes=\(nodeCount), labels=\(labelledCount), controls=\(buttonCount), disabled=\(disabledCount)")
