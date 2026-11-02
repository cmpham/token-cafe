import Foundation
import Security
import CommonCrypto
import LocalAuthentication

enum QuotaStorageError: Error {
    case unavailable, keychain(OSStatus), invalidCache, ambiguousAccount
}

// Read only Claude's existing Safe Storage item. Normal polls never open a
// password dialog; the explicit authorize command is the single interactive path.
func claudeStoragePassword(interactive: Bool) throws -> Data {
    let context = LAContext()
    context.interactionNotAllowed = !interactive
    let query: [String: Any] = [
        kSecClass as String: kSecClassGenericPassword,
        kSecAttrService as String: "Claude Safe Storage",
        kSecReturnData as String: true,
        kSecMatchLimit as String: kSecMatchLimitOne,
        kSecUseAuthenticationContext as String: context
    ]
    var item: CFTypeRef?
    let status = SecItemCopyMatching(query as CFDictionary, &item)
    guard status == errSecSuccess, let password = item as? Data else { throw QuotaStorageError.keychain(status) }
    return password
}

// Electron's macOS v10 Safe Storage format. Reject future formats rather than
// guessing. Plaintext exists only in this short-lived helper and the parent's pipe.
func decryptClaudeCache(_ encrypted: Data, password: Data) throws -> Data {
    guard encrypted.count > 3, encrypted.prefix(3) == Data("v10".utf8),
          encrypted.count <= 1_048_576, (encrypted.count - 3) % kCCBlockSizeAES128 == 0, !password.isEmpty else { throw QuotaStorageError.invalidCache }
    let salt = Data("saltysalt".utf8)
    var key = Data(count: kCCKeySizeAES128)
    let derived = key.withUnsafeMutableBytes { keyBytes in
        password.withUnsafeBytes { passwordBytes in
            salt.withUnsafeBytes { saltBytes in
                CCKeyDerivationPBKDF(CCPBKDFAlgorithm(kCCPBKDF2),
                    passwordBytes.baseAddress!.assumingMemoryBound(to: Int8.self), password.count,
                    saltBytes.baseAddress!.assumingMemoryBound(to: UInt8.self), salt.count,
                    CCPseudoRandomAlgorithm(kCCPRFHmacAlgSHA1), 1003,
                    keyBytes.baseAddress!.assumingMemoryBound(to: UInt8.self), kCCKeySizeAES128)
            }
        }
    }
    guard derived == kCCSuccess else { throw QuotaStorageError.invalidCache }
    let cipher = encrypted.dropFirst(3)
    let iv = Data(repeating: 0x20, count: kCCBlockSizeAES128)
    var plain = Data(count: cipher.count + kCCBlockSizeAES128)
    let capacity = plain.count
    var written = 0
    let status = plain.withUnsafeMutableBytes { destination in
        key.withUnsafeBytes { keyBytes in
            iv.withUnsafeBytes { ivBytes in
                cipher.withUnsafeBytes { source in
                    CCCrypt(CCOperation(kCCDecrypt), CCAlgorithm(kCCAlgorithmAES), CCOptions(kCCOptionPKCS7Padding),
                        keyBytes.baseAddress, key.count, ivBytes.baseAddress, source.baseAddress, cipher.count,
                        destination.baseAddress, capacity, &written)
                }
            }
        }
    }
    key.resetBytes(in: 0..<key.count)
    guard status == kCCSuccess else { throw QuotaStorageError.invalidCache }
    plain.count = written
    return plain
}

// Scope keys bind account, org, client, API host and OAuth scope. Never choose by
// email, or silently choose another organization when the requested one is absent.
func selectClaudeCredential(_ cache: [String: Any], account: String, org: String?) throws -> [String: Any] {
    guard UUID(uuidString: account) != nil else { throw QuotaStorageError.invalidCache }
    let prefix = "acct:\(account)|"
    var candidates: [(String, [String: Any])] = []
    for (scope, item) in cache where scope.hasPrefix(prefix) {
        let tail = String(scope.dropFirst(prefix.count))
        let fields = tail.components(separatedBy: ":https://api.anthropic.com:")
        guard fields.count == 2 else { continue }
        let ids = fields[0].split(separator: ":", omittingEmptySubsequences: false)
        let scopes = Set(fields[1].split(separator: " "))
        guard ids.count == 2, UUID(uuidString: String(ids[0])) != nil,
              UUID(uuidString: String(ids[1])) != nil,
              scopes.isSuperset(of: ["user:profile", "user:inference", "user:sessions:claude_code"]),
              let value = item as? [String: Any], let token = value["token"] as? String,
              !token.isEmpty, token.utf8.allSatisfy({ $0 > 32 && $0 < 127 }),
              let expires = value["expiresAt"] as? NSNumber,
              expires.doubleValue.isFinite, expires.doubleValue > 0 else { continue }
        let candidateOrg = String(ids[1])
        if let org, candidateOrg != org { continue }
        candidates.append((candidateOrg, ["token": token, "expiresAt": expires, "account": account, "org": candidateOrg]))
    }
    guard Set(candidates.map { $0.0 }).count <= 1 else { throw QuotaStorageError.ambiguousAccount }
    guard let chosen = candidates.max(by: {
        ($0.1["expiresAt"] as! NSNumber).doubleValue < ($1.1["expiresAt"] as! NSNumber).doubleValue
    }) else { throw QuotaStorageError.unavailable }
    return chosen.1
}

func currentClaudeCredential(org: String?) throws -> [String: Any] {
    let path = FileManager.default.homeDirectoryForCurrentUser
        .appendingPathComponent("Library/Application Support/Claude/config.json")
    let attributes = try FileManager.default.attributesOfItem(atPath: path.path)
    guard let size = attributes[.size] as? NSNumber, size.intValue <= 1_048_576 else { throw QuotaStorageError.invalidCache }
    let configData = try Data(contentsOf: path)
    guard configData.count <= 1_048_576,
          let config = try JSONSerialization.jsonObject(with: configData) as? [String: Any],
          let account = config["lastKnownAccountUuid"] as? String,
          let encoded = config["oauth:tokenCacheV2"] as? String,
          let encrypted = Data(base64Encoded: encoded) else { throw QuotaStorageError.unavailable }
    let password = try claudeStoragePassword(interactive: false)
    let plain = try decryptClaudeCache(encrypted, password: password)
    guard let cache = try JSONSerialization.jsonObject(with: plain) as? [String: Any] else { throw QuotaStorageError.invalidCache }
    return try selectClaudeCredential(cache, account: account, org: org)
}
