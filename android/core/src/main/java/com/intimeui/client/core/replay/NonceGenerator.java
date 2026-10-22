package com.intimeui.client.core.replay;

import java.security.SecureRandom;

/** CSPRNG-backed nonce generator: {@code n_} + 32 lowercase hex chars (128 bits). */
public final class NonceGenerator {
    private static final char[] HEX = "0123456789abcdef".toCharArray();
    private final SecureRandom random;

    public NonceGenerator() {
        this(new SecureRandom());
    }

    public NonceGenerator(SecureRandom random) {
        this.random = random;
    }

    public String newNonce() {
        byte[] bytes = new byte[16];
        random.nextBytes(bytes);
        char[] output = new char[2 + bytes.length * 2];
        output[0] = 'n';
        output[1] = '_';
        for (int index = 0; index < bytes.length; index++) {
            int value = bytes[index] & 0xff;
            output[2 + index * 2] = HEX[value >>> 4];
            output[3 + index * 2] = HEX[value & 0x0f];
        }
        return new String(output);
    }
}
