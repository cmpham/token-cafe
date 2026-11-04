// Pinned-session navigation reads only sidebar controls and optional Code metadata.
// Never uses recent-history shortcuts, composer keys, or a café selection.
import AppKit
import ApplicationServices
import CryptoKit

enum PinError: Error { case unavailable, ambiguous, limit }
struct PinnedCodeSession: Equatable {
    let id: String
    let title: String
}
struct PinRow {
    let title: String
    let current: Bool
}

func pinnedDestination(count: Int, current: [Int], forward: Bool) throws -> Int {
    guard count > 0, count <= 128 else { throw PinError.unavailable }
    guard current.count <= 1, current.allSatisfy({ (0..<count).contains($0) }) else { throw PinError.ambiguous }
    guard let index = current.first else { return forward ? 0 : count - 1 }
    return (index + (forward ? 1 : count - 1)) % count
}

func sessionURLMatches(_ url: String, id: String) -> Bool {
    guard !id.isEmpty, let decoded = url.removingPercentEncoding else { return false }
    let separators = CharacterSet.alphanumerics.union(CharacterSet(charactersIn: "_-")).inverted
    return decoded.components(separatedBy: separators).contains(id)
}

/// The sidebar defines membership and order. Disk metadata is only an optional
/// hint for the active row: cloud pins and stale local records must not disappear.
func claudePinnedRows(_ rows: [PinRow], sessions: [PinnedCodeSession], urls: [String]) throws -> [(Int, Bool)] {
    let byTitle = Dictionary(grouping: sessions, by: { normalizedTitle($0.title) })
    let rowTitles = Dictionary(grouping: rows.indices, by: { normalizedTitle(rows[$0].title) })
    return rows.enumerated().map { index, row in
        let title = normalizedTitle(row.title)
        let matches = byTitle[title] ?? []
        let active = rowTitles[title]?.count == 1 && Set(matches.map { $0.id }).count == 1 &&
            matches.contains { session in urls.contains { sessionURLMatches($0, id: session.id) } }
        return (index, row.current || active)
    }
}

func pinPageTitleMatches(_ title: String, row: String) -> Bool {
    let title = normalizedTitle(title), row = normalizedTitle(row)
    return !row.isEmpty && [row, row + " - Claude", row + " | Claude", row + " - Claude Code", row + " | Claude Code"].contains(title)
}

// Persist only hashes, never session names, URLs, or credentials. This bridges
// Claude rows that expose neither selected state nor a local session record.
func pinFingerprint(_ values: [String]) -> String {
    let data = (try? JSONEncoder().encode(values)) ?? Data()
    return SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined()
}
struct PinCursor: Codable {
    let process: Int32
    let rows: String
    let route: String
    let index: Int

    func matches(process: Int32, rows: [String], urls: [String]) -> Bool {
        self.process == process && self.rows == pinFingerprint(rows) &&
            !urls.filter({ !$0.isEmpty }).isEmpty && route == pinFingerprint(urls) && rows.indices.contains(index)
    }
}

func pinnedCodeSession(_ object: [String: Any]) -> PinnedCodeSession? {
    guard object["isArchived"] as? Bool != true,
          let id = object["sessionId"] as? String, !id.isEmpty,
          let title = object["title"] as? String, !normalizedTitle(title).isEmpty else { return nil }
    return PinnedCodeSession(id: id, title: title)
}

func readPinnedCodeSessions(at directory: URL) throws -> [PinnedCodeSession] {
    let keys: Set<URLResourceKey> = [.isRegularFileKey, .isDirectoryKey, .isSymbolicLinkKey, .fileSizeKey]
    guard let files = FileManager.default.enumerator(at: directory, includingPropertiesForKeys: Array(keys),
                                                    options: [.skipsHiddenFiles]) else { throw PinError.unavailable }
    var result: [PinnedCodeSession] = [], visited = 0
    while let url = files.nextObject() as? URL {
        visited += 1
        guard visited <= 4096 else { throw PinError.limit }
        let values = try url.resourceValues(forKeys: keys)
        if values.isSymbolicLink == true { continue }
        if files.level > 3 {
            if values.isDirectory == true { files.skipDescendants() }
            continue
        }
        guard values.isRegularFile == true, url.pathExtension == "json" else { continue }
        guard let size = values.fileSize, size <= 2 * 1024 * 1024 else { throw PinError.limit }
        let data = try Data(contentsOf: url)
        // An in-flight save must not produce a partial pin list.
        guard let object = try JSONSerialization.jsonObject(with: data) as? [String: Any] else { throw PinError.unavailable }
        if let session = pinnedCodeSession(object) { result.append(session) }
    }
    return result
}

func axParent(_ element: AXUIElement) -> AXUIElement? {
    guard let value = attribute(element, kAXParentAttribute), CFGetTypeID(value) == AXUIElementGetTypeID() else { return nil }
    return unsafeBitCast(value, to: AXUIElement.self)
}
func axURL(_ element: AXUIElement) -> String {
    let value = attribute(element, kAXURLAttribute)
    return (value as? URL)?.absoluteString ?? (value as? String) ?? ""
}
func pinName(_ element: AXUIElement) -> String {
    for key in [kAXTitleAttribute, kAXDescriptionAttribute] {
        let text = normalizedTitle(string(element, key))
        if !text.isEmpty { return text }
    }
    return ""
}
func claudeSessionName(rowName: String, menuNames: [String]) -> String {
    let prefix = "More options for "
    let titles = menuNames.filter { $0.hasPrefix(prefix) }.map { normalizedTitle(String($0.dropFirst(prefix.count))) }
    return titles.count == 1 && !titles[0].isEmpty ? titles[0] : rowName
}

func pinSessionName(_ element: AXUIElement, bundle: String) -> String {
    let name = pinName(element)
    guard bundle == claudeBundle, let parent = axParent(element) else { return name }
    // Claude nests the row menu one group below the row wrapper. Never descend
    // through the session button (its label includes fork/status decorations).
    var menus: [String] = []
    var pending = elements(parent).map { ($0, 0) }
    var visited = 0
    while let (node, depth) = pending.popLast(), visited < 24 {
        visited += 1
        let role = string(node, kAXRoleAttribute)
        if role == "AXPopUpButton" { menus.append(pinName(node)) }
        if role == "AXGroup" && depth < 2 {
            pending.append(contentsOf: elements(node).map { ($0, depth + 1) })
        }
    }
    return claudeSessionName(rowName: name, menuNames: menus)
}

func domClasses(_ element: AXUIElement) -> [String] {
    (attribute(element, "AXDOMClassList") as? [String]) ?? []
}
func pinRowCurrent(bundle: String, selected: Bool, ariaCurrent: String, classes: [String]) -> Bool {
    selected || ariaCurrent == "page"
}
func isPinnedSessionRow(bundle: String, classes: [String]) -> Bool {
    if bundle == codexBundle {
        return classes.contains("sidebar-item") &&
            classes.contains("data-[app-action-sidebar-thread-selected=true]:bg-primary-ghost-hover")
    }
    return bundle == claudeBundle && classes.contains("hit-row-main") &&
        classes.contains("ps-[calc(var(--df-row-px)+var(--df-row-indent,0px))]")
}

func pinContainerRole(_ role: String) -> Bool {
    ["AXWindow", "AXGroup", "AXWebArea", "AXScrollArea", "AXHeading", "AXList"].contains(role)
}

/// Walk containers only; never enter the transcript/composer, read text leaves,
/// or dump the window. Exhaustion cancels instead of returning a partial list.
func pinContainers(_ root: AXUIElement, budget: inout Int, depth: Int = 0,
                   visit: (AXUIElement) -> Bool) throws -> AXUIElement? {
    guard budget > 0, depth < 32 else { throw PinError.limit }
    budget -= 1
    let role = string(root, kAXRoleAttribute), subrole = string(root, kAXSubroleAttribute)
    guard !["AXLandmarkMain", "AXLandmarkRegion", "AXLandmarkBanner"].contains(subrole) else { return nil }
    if visit(root) { return root }
    guard pinContainerRole(role) else { return nil }
    for child in elements(root) {
        if let found = try pinContainers(child, budget: &budget, depth: depth + 1, visit: visit) { return found }
    }
    return nil
}

struct PinnedControls {
    let controls: [AXUIElement]
    let rows: [PinRow]
    let urls: [String]
    let webAreas: [AXUIElement]
    let identities: [String]
    let context: [String]
}

func pinContext(bundle: String, urls: [String], titles: [String]) -> [String] {
    // Claude updates its page title after navigation and can recreate AX nodes.
    // Its session URL is the stable identity; Codex also needs its page title.
    canonicalContext(bundle == claudeBundle ? urls : urls + titles)
}

func pinPageContext(_ webAreas: [AXUIElement], bundle: String) -> [String] {
    pinContext(bundle: bundle, urls: webAreas.map(axURL), titles: webAreas.map(pinName))
}

func pinVerticalPosition(_ control: AXUIElement) -> CGFloat {
    guard let raw = attribute(control, kAXPositionAttribute), CFGetTypeID(raw) == AXValueGetTypeID() else { return .greatestFiniteMagnitude }
    var point = CGPoint.zero
    guard AXValueGetValue(unsafeBitCast(raw, to: AXValue.self), .cgPoint, &point) else { return .greatestFiniteMagnitude }
    return point.y
}

func orderedPins(_ controls: [AXUIElement]) -> [AXUIElement] {
    // Read geometry once per row, rather than issuing AX calls from the sorter.
    let positions: [(index: Int, control: AXUIElement, y: CGFloat)] = controls.enumerated().map {
        (index: $0.offset, control: $0.element, y: pinVerticalPosition($0.element))
    }
    let sorted = positions.sorted { left, right in
        left.y == right.y ? left.index < right.index : left.y < right.y
    }
    return sorted.map { $0.control }
}

func pinIdentity(_ control: AXUIElement, bundle: String) -> String {
    if bundle == claudeBundle { return "title:" + pinSessionName(control, bundle: bundle) }
    if let node = attribute(control, "ChromeAXNodeId") as? NSNumber {
        return "node:" + node.stringValue
    }
    return "title:" + pinName(control)
}
func pinnedControls(window: AXUIElement, bundle: String) throws -> PinnedControls {
    var budget = 512, urls: [String] = [], pageTitles: [String] = [], webAreas: [AXUIElement] = []
    guard let sidebar = try pinContainers(window, budget: &budget, visit: { element in
        if string(element, kAXRoleAttribute) == "AXWebArea" {
            urls.append(axURL(element)); pageTitles.append(pinName(element)); webAreas.append(element)
        }
        return string(element, kAXSubroleAttribute) == "AXLandmarkComplementary" ||
            (string(element, kAXRoleAttribute) == "AXGroup" && pinName(element) == "Sidebar")
    }) else { throw PinError.unavailable }
    budget = 512
    guard let header = try pinContainers(sidebar, budget: &budget, visit: { element in
        ["AXHeading", "AXButton"].contains(string(element, kAXRoleAttribute)) && pinName(element) == "Pinned"
    }), let parent = axParent(header),
          let section = bundle == claudeBundle ? axParent(parent) : parent else { throw PinError.unavailable }
    var controls: [AXUIElement] = []
    budget = 512
    _ = try pinContainers(section, budget: &budget, visit: { element in
        // Exact row markers exclude the section toggle, menus and project rows.
        let classes = domClasses(element)
        if ["AXButton", "AXLink"].contains(string(element, kAXRoleAttribute)),
           !CFEqual(element, header), axParent(element).map({ !CFEqual($0, header) }) == true,
           isPinnedSessionRow(bundle: bundle, classes: classes) {
            controls.append(element)
        }
        return false
    })
    guard controls.count <= 128 else { throw PinError.limit }
    controls = orderedPins(controls)
    let rows = controls.map { element in
        let current = pinRowCurrent(bundle: bundle, selected: (attribute(element, kAXSelectedAttribute) as? Bool) == true,
                                    ariaCurrent: string(element, "AXARIACurrent"), classes: domClasses(element))
        let title = pinSessionName(element, bundle: bundle)
        return PinRow(title: title, current: current || pageTitles.contains { pinPageTitleMatches($0, row: title) })
    }
    return PinnedControls(controls: controls, rows: rows, urls: urls, webAreas: webAreas,
                          identities: controls.map { pinIdentity($0, bundle: bundle) }, context: pinPageContext(webAreas, bundle: bundle))
}

func navigatePinned(window: AXUIElement, root: AXUIElement, app: NSRunningApplication,
                    bundle: String, forward: Bool) throws {
    let started = Date()
    let before = try pinnedControls(window: window, bundle: bundle)
    let defaults = UserDefaults(suiteName: "local.token-cafe.desktop.pins")
    var current = before.rows.indices.filter { before.rows[$0].current }
    var source = "page"
    if let data = defaults?.data(forKey: bundle),
       let cursor = try? JSONDecoder().decode(PinCursor.self, from: data),
       cursor.matches(process: app.processIdentifier, rows: before.identities, urls: before.context) {
        current = [cursor.index]
        source = "cursor"
    } else if current.isEmpty && bundle == claudeBundle {
        // Only cold/manual navigation needs the optional metadata fallback.
        let directory = FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent("Library/Application Support/Claude/claude-code-sessions")
        let candidates = try claudePinnedRows(before.rows, sessions: (try? readPinnedCodeSessions(at: directory)) ?? [], urls: before.urls)
        current = candidates.indices.filter { candidates[$0].1 }
        source = "metadata"
    }
    let targetIndex = try pinnedDestination(count: before.rows.count, current: current, forward: forward)
    if before.rows.count == 1 && !current.isEmpty { return }
    func stillFocused() -> Bool {
        NSWorkspace.shared.frontmostApplication?.processIdentifier == app.processIdentifier &&
            focusedWindow(root).map({ CFEqual($0, window) }) == true
    }
    guard stillFocused() else { throw PinError.unavailable }
    func validTarget() -> Bool {
        stillFocused() && pinPageContext(before.webAreas, bundle: bundle) == before.context &&
            orderedPins(before.controls).map { pinIdentity($0, bundle: bundle) } == before.identities &&
            (attribute(before.controls[targetIndex], kAXEnabledAttribute) as? Bool) != false
    }
    let provider = bundle == codexBundle ? "codex" : "claude"
    fputs("Keypad pins: provider=\(provider) rows=\(before.rows.count) from=\(current.first ?? -1) to=\(targetIndex) source=\(source) lookup-ms=\(Int(Date().timeIntervalSince(started) * 1000))\n", stderr)
    // Visible rows need no scroll or unconditional wait. A rejected hit-test
    // posts no click, so scrolling and retrying that hit-test is safe.
    var clicked = clickControl(before.controls[targetIndex], root: root, validate: validTarget)
    if !clicked {
        guard validTarget(),
              AXUIElementPerformAction(before.controls[targetIndex], "AXScrollToVisible" as CFString) == .success else { throw PinError.unavailable }
        pauseForAppUpdate()
        clicked = clickControl(before.controls[targetIndex], root: root, validate: validTarget)
    }
    guard clicked else { throw PinError.unavailable }
    fputs("Keypad pins: provider=\(provider) click-ms=\(Int(Date().timeIntervalSince(started) * 1000))\n", stderr)
    let deadline = Date().addingTimeInterval(1.2)
    var candidateContext: [String] = []
    var candidateSince = Date()
    while Date() < deadline {
        RunLoop.current.run(until: Date().addingTimeInterval(0.04))
        guard stillFocused() else { throw PinError.unavailable }
        // Read just page identity after a click, not the entire sidebar each time.
        let context = pinPageContext(before.webAreas, bundle: bundle)
        let titles = before.webAreas.map(pinName)
        let exactTarget = titles.contains { pinPageTitleMatches($0, row: before.rows[targetIndex].title) }
        if context != before.context && (bundle == claudeBundle || exactTarget) {
            if bundle == claudeBundle {
                if candidateContext != context {
                    candidateContext = context
                    candidateSince = Date()
                    continue
                }
                // Avoid recording an intermediate route during page hydration.
                guard Date().timeIntervalSince(candidateSince) >= 0.12 else { continue }
            }
            let cursor = PinCursor(process: app.processIdentifier, rows: pinFingerprint(before.identities),
                                   route: pinFingerprint(context), index: targetIndex)
            defaults?.set(try JSONEncoder().encode(cursor), forKey: bundle)
            fputs("Keypad pins: navigation-confirmed provider=\(provider) total-ms=\(Int(Date().timeIntervalSince(started) * 1000))\n", stderr)
            return
        }
    }
    // A posted click is never retried. Do not use hover styling as confirmation.
    throw PinError.unavailable
}
