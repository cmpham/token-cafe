import Foundation

var checks = 0
func expect(_ condition: @autoclosure () -> Bool, _ message: String) {
    checks += 1
    if !condition() { fatalError(message) }
}
func rejects(_ block: () throws -> Void) -> Bool {
    do { try block(); return false } catch { return true }
}
let fixture = Data(base64Encoded: "djEw19kP1MJy4qAxYrsppYQka8+ywr+9JNkpbhNkRLALZbY=")!
let decoded = try decryptClaudeCache(fixture, password: Data("fictional-password".utf8))
expect(decoded == Data("{\"fixture\":true}".utf8), "independent OpenSSL fixture")
expect(rejects { _ = try JSONSerialization.jsonObject(with: decryptClaudeCache(fixture, password: Data("wrong".utf8))) }, "wrong storage key never yields a credential object")
for data in [Data(), Data("v10".utf8), Data("v11unsupported".utf8), Data("v10broken".utf8)] {
    expect(rejects { _ = try decryptClaudeCache(data, password: Data("fictional-password".utf8)) }, "malformed ciphertext")
}
let account = "11111111-1111-4111-8111-111111111111"
let otherAccount = "22222222-2222-4222-8222-222222222222"
let org = "33333333-3333-4333-8333-333333333333"
let otherOrg = "44444444-4444-4444-8444-444444444444"
let client = "55555555-5555-4555-8555-555555555555"
let scopes = "user:profile user:inference user:sessions:claude_code"
func key(_ account: String, _ org: String, _ host: String = "https://api.anthropic.com", _ scope: String = scopes) -> String {
    "acct:\(account)|\(client):\(org):\(host):\(scope)"
}
func token(_ value: String, _ expiry: Int) -> [String: Any] {
    ["token":value, "expiresAt":expiry, "refreshToken":"must-not-be-returned", "email":"same@example.invalid"]
}
let first = token("fictional-first", 2000000)
var cache: [String: Any] = [key(account, org):first, key(otherAccount, org):token("wrong-account", 9999999)]
var chosen = try selectClaudeCredential(cache, account: account, org: org)
expect(chosen["token"] as? String == "fictional-first", "active account only")
expect(chosen["refreshToken"] == nil && chosen["email"] == nil, "only quota credential fields leave the helper")
expect(chosen["account"] as? String == account && chosen["org"] as? String == org, "identity binding")
cache[key(account, org, "https://api.anthropic.com", scopes + " user:plugins")] = token("fictional-newer", 3000000)
chosen = try selectClaudeCredential(cache, account: account, org: org)
expect(chosen["token"] as? String == "fictional-newer", "newest matching credential")
cache[key(account, otherOrg)] = token("other-org", 9999999)
expect(rejects { _ = try selectClaudeCredential(cache, account: account, org: nil) }, "ambiguous org fails closed")
chosen = try selectClaudeCredential(cache, account: account, org: org)
expect(chosen["token"] as? String == "fictional-newer", "explicit organization")
expect(rejects { _ = try selectClaudeCredential(cache, account: account, org: "66666666-6666-4666-8666-666666666666") }, "missing org never borrows another")
for bad in [
    [key(account, org, "https://untrusted.invalid"):first],
    [key(account, org, "https://api.anthropic.com", "user:profile"):first],
    [key(account, org):NSNull()],
    [key(account, org):token("injected\nheader", 3000000)],
    [key(account, org):token("", 3000000)]
] as [[String: Any]] {
    expect(rejects { _ = try selectClaudeCredential(bad, account: account, org: org) }, "invalid or unrelated token")
}
expect(rejects { _ = try selectClaudeCredential(cache, account: "invalid", org: org) }, "invalid active identity")
print("PASS: read-only Claude quota cache (\(checks) checks; fictional data, no Keychain access)")
