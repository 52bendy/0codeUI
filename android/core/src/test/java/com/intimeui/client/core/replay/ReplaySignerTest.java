package com.intimeui.client.core.replay;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertThrows;

import org.junit.Test;

import java.nio.charset.StandardCharsets;

public class ReplaySignerTest {
    private static final String SECRET = "0123456789abcdef0123456789abcdef";
    private static final long TIMESTAMP = 1761111111L;
    private static final String NONCE = "n_00112233445566778899aabbccddeeff";
    private static final String USER_ID = "usr_9527";
    private static final String BODY =
            "{\"page_id\":\"ORDER_DETAIL_PAGE\",\"dwell_time\":1200,"
                    + "\"element_click_rate\":0.5,\"misclick_rate\":0.0,\"scroll_depth\":0.75}";

    @Test
    public void matchesProtocolReferenceVector() {
        ReplayHeaders headers = new ReplaySigner().sign(
                SECRET,
                "POST",
                "/api/v1/telemetry/event",
                TIMESTAMP,
                NONCE,
                USER_ID,
                BODY.getBytes(StandardCharsets.UTF_8));

        assertEquals("1761111111", headers.timestamp());
        assertEquals(NONCE, headers.nonce());
        assertEquals(
                "338d76fe638b9b8e10ffad9417e8820ab11da4fbcd1a229706f51637ac7b636c",
                headers.dynamicToken());
        assertEquals(
                "5a1b5d9a5bbcd0e01d033f3f73bcd81e5ed66b9bbd9dbfdda945b6b85c977ee2",
                headers.signature());
        assertEquals(4, headers.asMap().size());
    }

    @Test
    public void bodyBytesAreSignedExactly() {
        ReplayHeaders compact = new ReplaySigner().sign(
                SECRET, "POST", "/api/v1/telemetry/event", TIMESTAMP, NONCE, USER_ID,
                "{\"a\":1}".getBytes(StandardCharsets.UTF_8));
        ReplayHeaders spaced = new ReplaySigner().sign(
                SECRET, "POST", "/api/v1/telemetry/event", TIMESTAMP, NONCE, USER_ID,
                "{\"a\": 1}".getBytes(StandardCharsets.UTF_8));
        assertEquals(ReplaySigner.sha256Hex("{\"a\":1}".getBytes(StandardCharsets.UTF_8)),
                ReplaySigner.sha256Hex("{\"a\":1}".getBytes(StandardCharsets.UTF_8)));
        org.junit.Assert.assertNotEquals(compact.signature(), spaced.signature());
    }

    @Test
    public void rejectsMalformedInputs() {
        ReplaySigner signer = new ReplaySigner();
        assertThrows(IllegalArgumentException.class, () -> signer.sign(
                SECRET, "post", "/x", TIMESTAMP, NONCE, USER_ID, new byte[0]));
        assertThrows(IllegalArgumentException.class, () -> signer.sign(
                SECRET, "POST", "x", TIMESTAMP, NONCE, USER_ID, new byte[0]));
        assertThrows(IllegalArgumentException.class, () -> signer.sign(
                SECRET, "POST", "/x", TIMESTAMP, "short", USER_ID, new byte[0]));
        assertThrows(IllegalArgumentException.class, () -> signer.sign(
                "", "POST", "/x", TIMESTAMP, NONCE, USER_ID, new byte[0]));
    }
}
