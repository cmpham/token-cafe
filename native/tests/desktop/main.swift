import AppKit
import ApplicationServices

    precondition(voiceLabelMatches(" Start\nVoice Chat ", labels: ["Start voice chat"]))
    precondition(!voiceLabelMatches("Stop voice chat", labels: ["Start voice chat"]))
    precondition(!voiceLabelMatches("Start new voice chat", labels: ["Start voice chat"]))
    precondition(!voiceLabelMatches("", labels: ["Start voice chat"]))
    let codexLabels = recordingLabels(for: codexBundle)
    precondition(voiceLabelMatches("Dictate", labels: codexLabels))
    precondition(!voiceLabelMatches("Start voice chat", labels: codexLabels))
    precondition(!voiceLabelMatches("Start Voice", labels: codexLabels))
    precondition(!voiceLabelMatches("Stop dictation", labels: codexLabels))
    precondition(voiceLabelMatches("Press and hold to record", labels: recordingLabels(for: claudeBundle)))
    precondition(supportsRecording(codexBundle))
    precondition(supportsRecording(claudeBundle))
    precondition(!supportsRecording("com.apple.Terminal"))
    precondition(!supportsRecording("local.token-cafe.desktop"))
    precondition(canonicalContext(["route:a", "title:b", "", "route:a"]) == ["route:a", "title:b"])
    precondition(canonicalContext(["title:b", "route:a"]) == canonicalContext(["route:a", "title:b"]))
    precondition(canonicalContext(["route:a"]) != canonicalContext(["route:b"]))
    precondition(try! pinnedDestination(count: 3, current: [], forward: true) == 0)
    precondition(try! pinnedDestination(count: 3, current: [], forward: false) == 2)
    precondition(try! pinnedDestination(count: 3, current: [1], forward: true) == 2)
    precondition(try! pinnedDestination(count: 3, current: [1], forward: false) == 0)
    precondition(try! pinnedDestination(count: 3, current: [2], forward: true) == 0)
    precondition(try! pinnedDestination(count: 3, current: [0], forward: false) == 2)
    precondition(try! pinnedDestination(count: 1, current: [0], forward: true) == 0)
    func rejects(_ body: () throws -> Void) {
        do { try body(); preconditionFailure("Expected navigation to be rejected") } catch {}
    }
    rejects { _ = try pinnedDestination(count: 0, current: [], forward: true) }
    rejects { _ = try pinnedDestination(count: 129, current: [], forward: true) }
    rejects { _ = try pinnedDestination(count: 3, current: [0, 1], forward: true) }
    rejects { _ = try pinnedDestination(count: 3, current: [3], forward: true) }
    precondition(sessionURLMatches("https://claude.ai/code/local_moon?mode=code", id: "local_moon"))
    precondition(sessionURLMatches("https://claude.ai/code?session=local_moon", id: "local_moon"))
    precondition(sessionURLMatches("https://claude.ai/code/%6cocal_moon", id: "local_moon"))
    precondition(!sessionURLMatches("https://claude.ai/code/local_moon-other", id: "local_moon"))
    precondition(!sessionURLMatches("https://claude.ai/code/local_moonlight", id: "local_moon"))
    precondition(!sessionURLMatches("https://claude.ai/code/local_moon", id: ""))
    precondition(!pinRowCurrent(bundle: codexBundle, selected: false, ariaCurrent: "", classes: ["bg-primary-ghost-hover"]))
    precondition(!pinRowCurrent(bundle: codexBundle, selected: false, ariaCurrent: "", classes: ["hover:bg-primary-ghost-hover", "data-[selected=true]:bg-primary-ghost-hover"]))
    precondition(!pinRowCurrent(bundle: claudeBundle, selected: false, ariaCurrent: "", classes: ["bg-primary-ghost-hover"]))
    precondition(pinRowCurrent(bundle: claudeBundle, selected: false, ariaCurrent: "page", classes: []))
    precondition(isPinnedSessionRow(bundle: codexBundle, classes: ["sidebar-item", "data-[app-action-sidebar-thread-selected=true]:bg-primary-ghost-hover"]))
    precondition(!isPinnedSessionRow(bundle: codexBundle, classes: ["sidebar-item", "group/section-toggle"]))
    precondition(isPinnedSessionRow(bundle: claudeBundle, classes: ["hit-row-main", "ps-[calc(var(--df-row-px)+var(--df-row-indent,0px))]"]))
    precondition(!isPinnedSessionRow(bundle: claudeBundle, classes: ["hit-row-main", "px-[var(--df-row-px)]"]))
    precondition(!isPinnedSessionRow(bundle: "com.apple.Terminal", classes: ["sidebar-item"]))
    let records = [PinnedCodeSession(id: "local_moon", title: "Moon Café"), PinnedCodeSession(id: "local_sun", title: "Sun Café")]
    let rows = [PinRow(title: "Project", current: false), PinRow(title: "Sun Café", current: false),
                PinRow(title: "Moon\n Café", current: false)]
    let pins = try claudePinnedRows(rows, sessions: records, urls: ["https://claude.ai/code/local_moon"])
    precondition(pins.map { $0.0 } == [0, 1, 2]) // sidebar order, including cloud pins absent from disk
    precondition(pins.map { $0.1 } == [false, false, true]) // active identity, independent of café
    precondition(try! claudePinnedRows(rows, sessions: [], urls: []).count == rows.count)
    precondition(try! claudePinnedRows(rows, sessions: records + records, urls: []).count == rows.count)
    precondition(try! claudePinnedRows(rows + [rows[1]], sessions: records, urls: []).count == rows.count + 1)
    let saved: [String: Any] = ["sessionId": "local_moon", "title": "Moon Café", "isStarred": true]
    precondition(pinnedCodeSession(saved) == records[0])
    precondition(pinnedCodeSession(saved.merging(["isArchived": true]) { _, new in new }) == nil)
    precondition(pinnedCodeSession(saved.merging(["isStarred": false]) { _, new in new }) == records[0])
    precondition(pinnedCodeSession(["sessionId": "local_moon", "isStarred": true]) == nil)
    let directory = FileManager.default.temporaryDirectory.appendingPathComponent("token-cafe-pins-" + UUID().uuidString)
    try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
    defer { try? FileManager.default.removeItem(at: directory) }
    let nested = directory.appendingPathComponent("account/org")
    try FileManager.default.createDirectory(at: nested, withIntermediateDirectories: true)
    try JSONSerialization.data(withJSONObject: saved).write(to: nested.appendingPathComponent("moon.json"))
    precondition(try! readPinnedCodeSessions(at: directory) == [records[0]])
    try FileManager.default.createSymbolicLink(at: directory.appendingPathComponent("copy.json"),
                                             withDestinationURL: nested.appendingPathComponent("moon.json"))
    precondition(try! readPinnedCodeSessions(at: directory) == [records[0]])
    try Data("{".utf8).write(to: nested.appendingPathComponent("broken.json"))
    rejects { _ = try readPinnedCodeSessions(at: directory) }
    print("PASS: focused recording and pinned-session routing, wrap, identity, ambiguity and metadata fixtures (no app interaction)")

    precondition(pinContainerRole("AXList")) // Codex puts pins in AXContentList
    precondition(!pinContainerRole("AXTextArea"))
    precondition(!pinContainerRole("AXStaticText"))
    precondition(pinPageTitleMatches("Moon Café - Claude", row: "Moon Café"))
    precondition(!pinPageTitleMatches("Moon Café extra", row: "Moon Café"))
    let cursor = PinCursor(process: 12, rows: pinFingerprint(["A", "B"]), route: pinFingerprint(["route-a"]), index: 1)
    precondition(cursor.matches(process: 12, rows: ["A", "B"], urls: ["route-a"]))
    precondition(!cursor.matches(process: 13, rows: ["A", "B"], urls: ["route-a"]))
    precondition(!cursor.matches(process: 12, rows: ["B", "A"], urls: ["route-a"]))
    precondition(!cursor.matches(process: 12, rows: ["A", "B"], urls: ["route-b"]))
    precondition(!cursor.matches(process: 12, rows: ["A", "B"], urls: []))
    // Every pin participates, including missing/stale metadata, in both directions.
    let six = (0..<6).map { PinRow(title: "Pin \($0)", current: false) }
    let partial = [1, 3, 4].map { PinnedCodeSession(id: "s\($0)", title: six[$0].title) }
    let complete = try claudePinnedRows(six, sessions: partial, urls: [])
    precondition(complete.map { $0.0 } == Array(0..<6))
    for index in 0..<6 {
        precondition(try! pinnedDestination(count: complete.count, current: [index], forward: true) == (index + 1) % 6)
        precondition(try! pinnedDestination(count: complete.count, current: [index], forward: false) == (index + 5) % 6)
    }

    precondition(pinPageTitleMatches("Pinned task", row: "Pinned task"))
    precondition(!pinPageTitleMatches("Another task", row: "Pinned task"))

    // Reproduce top-pin hydration: changing the page title must not lose Claude's
    // cursor and send Down back to the already-selected first pin.
    let initialContext = pinContext(bundle: claudeBundle, urls: ["https://claude.ai/code/first"], titles: ["Loading"])
    let loadedContext = pinContext(bundle: claudeBundle, urls: ["https://claude.ai/code/first"], titles: ["First task"])
    let topCursor = PinCursor(process: 42, rows: pinFingerprint(["title:First", "title:Second"]),
                              route: pinFingerprint(initialContext), index: 0)
    precondition(topCursor.matches(process: 42, rows: ["title:First", "title:Second"], urls: loadedContext))
    precondition(try! pinnedDestination(count: 2, current: [topCursor.index], forward: true) == 1)
    let otherContext = pinContext(bundle: claudeBundle, urls: ["https://claude.ai/code/other"], titles: ["First task"])
    precondition(!topCursor.matches(process: 42, rows: ["title:First", "title:Second"], urls: otherContext))
    precondition(pinContext(bundle: codexBundle, urls: ["app://index"], titles: ["First"]) !=
                 pinContext(bundle: codexBundle, urls: ["app://index"], titles: ["Second"]))

    let cleanFork = claudeSessionName(rowName: "Review tools Forked from Original tools", menuNames: ["More options for Review tools"])
    precondition(cleanFork == "Review tools")
    precondition(pinPageTitleMatches("Review tools - Claude Code", row: cleanFork))
    precondition(claudeSessionName(rowName: "Review tools", menuNames: []) == "Review tools")
    precondition(claudeSessionName(rowName: "Review tools", menuNames: ["More options for A", "More options for B"]) == "Review tools")
    let coldRows = [PinRow(title: cleanFork, current: pinPageTitleMatches("Review tools - Claude Code", row: cleanFork)), PinRow(title: "Other", current: false)]
    let coldCurrent = coldRows.indices.filter { coldRows[$0].current }
    precondition(coldCurrent == [0])
    precondition(try! pinnedDestination(count: coldRows.count, current: coldCurrent, forward: true) == 1)
