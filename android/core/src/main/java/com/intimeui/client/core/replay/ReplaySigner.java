package com.intimeui.client.core.replay;

import java.nio.charset.StandardCharsets;
import java.security.GeneralSecurityException;
import java.security.MessageDigest;
import java.util.Locale;
import java.util.regex.Pattern;

import javax.crypto.Mac;
import javax.crypto.spec.SecretKeySpec;

/**
 * Client implementation of the INTIMEUI-REPLAY-V1 protocol.
 *
 * <p>The signer works on the exact bytes that are sent on the wire. Callers must
 * serialize the body once and reuse the same array for signing and transport.</p>
 */
public final class ReplaySigner {
    public static final String PROTOCOL = "INTIMEUI-REPLAY-V1";
    private static final String TOKEN_DOMAIN = "intimeui.replay.dynamic-token.v1";
    private static final Pattern NONCE_PATTERN =
            Pattern.compile("^[A-Za-z0-9._:-]{16,128}$");
    private static final Pattern METHOD_PATTERN = Pattern.compile("^[A-Z]{3,10}$");
    private static final char[] HEX = "0123456789abcdef".toCharArray();

    /**
     * Builds the four protocol headers.
     *
     * @throws IllegalArgumentException when any protocol input is malformed
     */
    public ReplayHeaders sign(
            String secret,
            String method,
            String pathAndQuery,
            long timestamp,
            String nonce,
            String userId,
            byte[] body) {
        requireText(secret, "secret");
        requireText(userId, "user_id");
        if (method == null || !METHOD_PATTERN.matcher(method).matches()) {
            throw new IllegalArgumentException("method must be an uppercase HTTP verb");
        }
        if (pathAndQuery == null || !pathAndQuery.startsWith("/")
                || pathAndQuery.indexOf('\n') >= 0 || pathAndQuery.indexOf('\r') >= 0) {
            throw new IllegalArgumentException("pathAndQuery must start with `/` and contain no newlines");
        }
        if (nonce == null || !NONCE_PATTERN.matcher(nonce).matches()) {
            throw new IllegalArgumentException("nonce must be 16..128 chars of [A-Za-z0-9._:-]");
        }
        if (timestamp <= 0) {
            throw new IllegalArgumentException("timestamp must be a positive Unix second value");
        }
        byte[] safeBody = body == null ? new byte[0] : body;

        String timestampText = Long.toString(timestamp);
        String dynamicToken = hmacSha256Hex(
                secret.getBytes(StandardCharsets.UTF_8),
                (TOKEN_DOMAIN + "\n" + timestampText + "\n" + nonce)
                        .getBytes(StandardCharsets.UTF_8));
        String bodySha256 = sha256Hex(safeBody);
        String canonical = String.join(
                "\n",
                PROTOCOL,
                method,
                pathAndQuery,
                timestampText,
                nonce,
                userId,
                bodySha256);
        String signature = hmacSha256Hex(
                dynamicToken.getBytes(StandardCharsets.UTF_8),
                canonical.getBytes(StandardCharsets.UTF_8));
        return new ReplayHeaders(timestampText, nonce, dynamicToken, signature);
    }

    /** Lowercase hex SHA-256 of the supplied bytes. */
    public static String sha256Hex(byte[] bytes) {
        try {
            MessageDigest digest = MessageDigest.getInstance("SHA-256");
            return toHex(digest.digest(bytes));
        } catch (GeneralSecurityException error) {
            throw new IllegalStateException("SHA-256 is unavailable", error);
        }
    }

    static String hmacSha256Hex(byte[] key, byte[] message) {
        try {
            Mac mac = Mac.getInstance("HmacSHA256");
            mac.init(new SecretKeySpec(key, "HmacSHA256"));
            return toHex(mac.doFinal(message));
        } catch (GeneralSecurityException error) {
            throw new IllegalStateException("HmacSHA256 is unavailable", error);
        }
    }

    private static String toHex(byte[] bytes) {
        char[] output = new char[bytes.length * 2];
        for (int index = 0; index < bytes.length; index++) {
            int value = bytes[index] & 0xff;
            output[index * 2] = HEX[value >>> 4];
            output[index * 2 + 1] = HEX[value & 0x0f];
        }
        return new String(output).toLowerCase(Locale.ROOT);
    }

    private static void requireText(String value, String field) {
        if (value == null || value.isEmpty()) {
            throw new IllegalArgumentException(field + " must not be empty");
        }
    }
}
