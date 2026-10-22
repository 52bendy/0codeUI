package com.intimeui.client.core.api;

/**
 * Supplies the short-lived replay signing secret provisioned after login.
 *
 * <p>Production builds must never embed the secret in the APK. The host app is
 * expected to fetch it through a secure channel and hand it to this provider.</p>
 */
public interface ReplaySecretProvider {
    /** Returns the current secret, or {@code null}/empty when signing is disabled. */
    String secret();
}
