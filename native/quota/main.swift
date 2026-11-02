import Foundation

let args = CommandLine.arguments
// authorize prints no credential. read is a private, pipe-only protocol with the
// companion; refuse a terminal to reduce accidental disclosure during debugging.
do {
    if args.count == 2 && args[1] == "authorize" {
        _ = try claudeStoragePassword(interactive: true)
        print("Claude quota Keychain access granted")
    } else if (2...3).contains(args.count), args[1] == "read", isatty(STDOUT_FILENO) == 0 {
        if args.count == 3, UUID(uuidString: args[2]) == nil { throw QuotaStorageError.invalidCache }
        let credential = try currentClaudeCredential(org: args.count == 3 ? args[2] : nil)
        FileHandle.standardOutput.write(try JSONSerialization.data(withJSONObject: credential))
    } else {
        throw QuotaStorageError.unavailable
    }
} catch let error as QuotaStorageError {
    switch error {
    case .keychain(let status): fputs("Claude quota: Keychain unavailable (\(status))\n", stderr)
    case .ambiguousAccount: fputs("Claude quota: organization selector required\n", stderr)
    default: fputs("Claude quota: current credential unavailable\n", stderr)
    }
    exit(1)
} catch {
    fputs("Claude quota: current credential unavailable\n", stderr)
    exit(1)
}
