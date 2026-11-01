// Native desktop integration invoked only by an authenticated physical keypad action.
// No transcript text is read, logged, or sent. Never submits a prompt.
import AppKit
import ApplicationServices

let codexBundle = "com.openai.codex"
let claudeBundle = "com.anthropic.claudefordesktop"
func fail(_ text: String) -> Never {
    fputs("Keypad: \(text)\n", stderr)
    exit(1)
}
func attribute(_ element: AXUIElement, _ key: String) -> CFTypeRef? {
    var value: CFTypeRef?
    guard AXUIElementCopyAttributeValue(element, key as CFString, &value) == .success else { return nil }
    return value
}
func string(_ element: AXUIElement, _ key: String) -> String {
    (attribute(element, key) as? String) ?? ""
}
func elements(_ element: AXUIElement) -> [AXUIElement] {
    (attribute(element, kAXChildrenAttribute) as? [AXUIElement]) ?? []
}
func walk(_ root: AXUIElement) -> [AXUIElement] {
    var result: [AXUIElement] = [], stack = [(root, 0)]
    while let (item, depth) = stack.popLast(), result.count < 6000 {
        result.append(item)
        if depth < 80 { stack.append(contentsOf: elements(item).reversed().map { ($0, depth + 1) }) }
    }
    return result
}
func identity(_ url: URL) -> String? {
    if url.scheme == "codex", url.host == "threads" { return url.pathComponents.last }
    if url.scheme == "claude", url.host == "code", url.path == "/continue" {
        return URLComponents(url: url, resolvingAgainstBaseURL: false)?.queryItems?.first { $0.name == "session" }?.value
    }
    return nil
}
func pauseForAppUpdate() {
    // NSWorkspace activation/focus updates arrive on the main run loop.
    // Sleeping this thread leaves its foreground-app cache stale after opening a link.
    RunLoop.current.run(until: Date().addingTimeInterval(0.15))
}
func normalizedTitle(_ text: String) -> String {
    text.split(whereSeparator: { $0.isWhitespace }).joined(separator: " ")
}
func titleMatches(_ candidate: String, _ expected: String) -> Bool {
    let title = normalizedTitle(expected)
    guard !title.isEmpty else { return false }
    let candidate = normalizedTitle(candidate)
    // The collector intentionally limits display names to 80 characters.
    return candidate == title || (title.count == 80 && candidate.count > 80 && String(candidate.prefix(80)) == title)
}
func documentSession(_ text: String) -> String? {
    guard let url = URL(string: text) else { return nil }
    let parts = url.pathComponents.filter { $0 != "/" }
    if parts.count == 2 && ["threads", "epitaxy"].contains(parts[0]) { return parts[1] }
    return identity(url)
}
func headingText(_ element: AXUIElement, depth: Int = 0) -> [String] {
    let own = [kAXTitleAttribute, kAXValueAttribute, kAXDescriptionAttribute].map { string(element, $0) }.filter { !$0.isEmpty }
    if !own.isEmpty || depth >= 2 { return own }
    return elements(element).flatMap { headingText($0, depth: depth + 1) }
}
func voiceLabelMatches(_ candidate: String, labels: [String]) -> Bool {
    let candidate = normalizedTitle(candidate).lowercased()
    return labels.contains { candidate == normalizedTitle($0).lowercased() }
}
func recordingLabels(for bundle: String) -> [String] {
    // The session composer exposes Dictate. Start Voice / Start voice chat
    // can launch the pet or a separate conversation, so never use them here.
    bundle == codexBundle ? ["Dictate"] : ["Start voice mode", "Start voice chat", "Press and hold to record"]
}
func voiceControlMatches(_ element: AXUIElement, labels: [String]) -> Bool {
    // Native controls can expose the accessible name as AXValue or as a static
    // text child, whereas Electron usually exposes it directly as AXTitle.
    let own = [kAXTitleAttribute, kAXDescriptionAttribute, kAXHelpAttribute, kAXValueAttribute]
        .map { string(element, $0) }
    if own.contains(where: { voiceLabelMatches($0, labels: labels) }) { return true }
    return elements(element).contains { child in
        ["AXStaticText", "AXGroup"].contains(string(child, kAXRoleAttribute)) &&
        headingText(child, depth: 1).contains { voiceLabelMatches($0, labels: labels) }
    }
}
func sessionMatches(_ nodes: [AXUIElement], id: String, title: String) -> Bool {
    let documents = nodes.filter { ["AXWebArea", "AXWindow"].contains(string($0, kAXRoleAttribute)) }
    let visibleRoutes = documents.flatMap { node in
        [kAXURLAttribute, kAXDocumentAttribute].compactMap { key -> String? in
            guard let value = attribute(node, key) else { return nil }
            let text = (value as? URL)?.absoluteString ?? (value as? String) ?? ""
            return documentSession(text)
        }
    }
    // A known different route cannot be overridden by a matching sidebar/title.
    if !visibleRoutes.isEmpty { return visibleRoutes.contains(id) }
    // Codex sets the native window/document title rather than a heading.
    // The host passes a title only when it uniquely identifies a live session.
    if documents.contains(where: { titleMatches(string($0, kAXTitleAttribute), title) }) { return true }
    return nodes.contains { node in
        string(node, kAXRoleAttribute) == "AXHeading" && headingText(node).contains { titleMatches($0, title) }
    }
}
