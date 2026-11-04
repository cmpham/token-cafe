import AppKit
import ApplicationServices

let args = CommandLine.arguments
if args.count == 2 && args[1] == "toggle" {
    let current = NSWorkspace.shared.frontmostApplication?.bundleIdentifier
    let target = current == claudeBundle ? codexBundle : claudeBundle
    guard let url = NSWorkspace.shared.urlForApplication(withBundleIdentifier: target) else { fail("Desktop app unavailable") }
    exit(NSWorkspace.shared.open(url) ? 0 : 1)
}
let pins = args.count == 3 && args[1] == "pinned-focused" && ["up", "down"].contains(args[2])
guard pins || (args.count == 2 && args[1] == "voice-focused") else { fail("Invalid command") }
guard let app = NSWorkspace.shared.frontmostApplication,
      let bundle = app.bundleIdentifier, supportsRecording(bundle) else {
    fail("Focus Codex or Claude to use this keypad action")
}
let trustOptions = [kAXTrustedCheckOptionPrompt.takeUnretainedValue() as String: true] as CFDictionary
guard AXIsProcessTrustedWithOptions(trustOptions) else { fail("Enable Accessibility for Token Café desktop controls") }
let root = AXUIElementCreateApplication(app.processIdentifier)
AXUIElementSetMessagingTimeout(root, 0.5)
guard let window = focusedWindow(root) else { fail("No focused window") }
// Expose Electron controls without opening a link or activating another app.
let result = AXUIElementSetAttributeValue(root, "AXManualAccessibility" as CFString, kCFBooleanTrue)
fputs("Keypad \(pins ? "pins" : "voice"): accessibility=\(result.rawValue) provider=\(bundle == codexBundle ? "codex" : "claude")\n", stderr)
pauseForAppUpdate()
guard NSWorkspace.shared.frontmostApplication?.processIdentifier == app.processIdentifier,
      let current = focusedWindow(root), CFEqual(current, window) else { fail("Focus changed") }
if pins {
    do {
        try navigatePinned(window: window, root: root, app: app, bundle: bundle, forward: args[2] == "down")
        exit(0)
    } catch {
        fail("Pinned navigation not confirmed (show the Pinned sidebar section and keep the app focused)")
    }
}
// The hosted accessibility tree may still be hydrating after AX enablement.
// Wait for a stable tree in this same window before binding its session context.
let initialWindowContext = windowContext([window])
var originalContext = windowContext(walk(window))
let warmupDeadline = Date().addingTimeInterval(1)
while Date() < warmupDeadline {
    pauseForAppUpdate()
    guard NSWorkspace.shared.frontmostApplication?.processIdentifier == app.processIdentifier,
          let current = focusedWindow(root), CFEqual(current, window),
          windowContext([window]) == initialWindowContext else { fail("Focus changed during accessibility setup") }
    let context = windowContext(walk(window))
    if context == originalContext { break }
    originalContext = context
}
let deadline = Date().addingTimeInterval(8)
var phase = "waiting-for-app"
var nodeCount = 0
var buttonCount = 0
var labelledCount = 0
var disabledCount = 0
while Date() < deadline {
    guard NSWorkspace.shared.frontmostApplication?.processIdentifier == app.processIdentifier,
          let current = focusedWindow(root), CFEqual(current, window) else { fail("Focus changed") }
    let nodes = walk(window)
    nodeCount = nodes.count
    phase = "waiting-for-session"
    guard windowContext(nodes) == originalContext else { fail("Focused session changed") }
    do {
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
            guard NSWorkspace.shared.frontmostApplication?.processIdentifier == app.processIdentifier,
                  let current = focusedWindow(root), CFEqual(current, window),
                  windowContext(walk(window)) == originalContext else { fail("Focus changed") }
            let activated = clickControl(buttons[0], root: root)
            if !activated {
                // A failed hit-test posts no events, so refreshing stale geometry
                // is safe. Keep handling this one physical press until ready.
                phase = "waiting-for-recording-hit-test"
                pauseForAppUpdate()
                continue
            }
            if bundle == codexBundle {
                // AXPress can report success without starting recording. Confirm the
                // composer changed state; never retry a press that could toggle it off.
                let confirmationDeadline = Date().addingTimeInterval(2)
                while Date() < confirmationDeadline {
                    pauseForAppUpdate()
                    guard NSWorkspace.shared.frontmostApplication?.processIdentifier == app.processIdentifier,
                          let current = focusedWindow(root), CFEqual(current, window) else { fail("Focus changed after activation") }
                    if walk(window).contains(where: { node in
                        ["AXButton", "AXCheckBox"].contains(string(node, kAXRoleAttribute)) &&
                        voiceControlMatches(node, labels: ["Stop dictation"])
                    }) {
                        fputs("Keypad voice: recording-control-confirmed\n", stderr)
                        exit(0)
                    }
                }
                fail("Dictate accepted activation but recording was not confirmed")
            }
            fputs("Keypad voice: claude-recording-click-sent\n", stderr)
            exit(0)
        }
    }
    pauseForAppUpdate()
}
fail("Native voice unavailable: \(phase), nodes=\(nodeCount), labels=\(labelledCount), controls=\(buttonCount), disabled=\(disabledCount)")
