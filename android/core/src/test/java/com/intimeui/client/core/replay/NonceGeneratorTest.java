package com.intimeui.client.core.replay;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertTrue;

import org.junit.Test;

import java.util.HashSet;
import java.util.Set;

public class NonceGeneratorTest {
    @Test
    public void emitsProtocolCompatibleNonces() {
        NonceGenerator generator = new NonceGenerator();
        Set<String> seen = new HashSet<>();
        for (int index = 0; index < 200; index++) {
            String nonce = generator.newNonce();
            assertEquals(34, nonce.length());
            assertTrue(nonce.matches("^n_[0-9a-f]{32}$"));
            assertTrue("nonce must be unique", seen.add(nonce));
        }
    }
}
