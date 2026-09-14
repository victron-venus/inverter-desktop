#import <Foundation/Foundation.h>
#import <Security/Security.h>
#include <stdint.h>
#include <string.h>

static NSString *const InstallationIdPreference = @"credential-key-installation-id-v1";
static NSString *const CredentialKeyService = @"com.alvit.inverter-dashboard.config-key.v1";

static OSStatus read_key(NSDictionary *identity, uint8_t *output) {
    NSMutableDictionary *query = [identity mutableCopy];
    query[(id)kSecReturnData] = @YES;
    query[(id)kSecMatchLimit] = (id)kSecMatchLimitOne;
    CFTypeRef result = NULL;
    OSStatus status = SecItemCopyMatching((CFDictionaryRef)query, &result);
    [query release];
    if (status == errSecSuccess) {
        if (result == NULL || CFGetTypeID(result) != CFDataGetTypeID() ||
            CFDataGetLength((CFDataRef)result) != 32) {
            status = errSecDecode;
        } else {
            memcpy(output, CFDataGetBytePtr((CFDataRef)result), 32);
        }
    }
    if (result != NULL) {
        CFRelease(result);
    }
    return status;
}

// Called only from Rust, never exposed as a webview command. NSUserDefaults
// contains a non-secret installation identifier; the 32-byte key lives only in
// Keychain. A fresh app installation uses a new account even though iOS may
// retain old Keychain entries after uninstalling an app.
int32_t inverter_mobile_encryption_key(uint8_t *output, size_t length) {
    if (output == NULL || length != 32) {
        return errSecParam;
    }
    memset(output, 0, length);
    @autoreleasepool {
        NSUserDefaults *preferences = [NSUserDefaults standardUserDefaults];
        @synchronized(preferences) {
            id savedId = [preferences objectForKey:InstallationIdPreference];
            NSString *installationId = nil;
            if (savedId != nil) {
                if (![savedId isKindOfClass:[NSString class]]) {
                    return errSecDecode;
                }
                NSUUID *uuid = [[NSUUID alloc] initWithUUIDString:savedId];
                if (uuid == nil) {
                    return errSecDecode;
                }
                [uuid release];
                installationId = savedId;
            } else {
                installationId = [[NSUUID UUID] UUIDString];
                [preferences setObject:installationId forKey:InstallationIdPreference];
                // Persist the non-secret identifier before creating its key.
                if (![preferences synchronize]) {
                    return errSecIO;
                }
            }

            NSDictionary *identity = @{
                (id)kSecClass: (id)kSecClassGenericPassword,
                (id)kSecAttrService: CredentialKeyService,
                (id)kSecAttrAccount: installationId,
                (id)kSecAttrSynchronizable: @NO,
            };
            OSStatus status = read_key(identity, output);
            if (status != errSecItemNotFound) {
                return status;
            }

            NSMutableData *key = [NSMutableData dataWithLength:32];
            status = SecRandomCopyBytes(kSecRandomDefault, key.length, key.mutableBytes);
            if (status != errSecSuccess) {
                return status;
            }
            NSMutableDictionary *item = [identity mutableCopy];
            item[(id)kSecAttrAccessible] = (id)kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly;
            item[(id)kSecValueData] = key;
            status = SecItemAdd((CFDictionaryRef)item, NULL);
            [item release];
            if (status == errSecSuccess) {
                memcpy(output, key.bytes, 32);
            }
            memset(key.mutableBytes, 0, key.length);
            // A concurrent creator must never cause us to return a different key.
            if (status == errSecDuplicateItem) {
                return read_key(identity, output);
            }
            return status;
        }
    }
}
