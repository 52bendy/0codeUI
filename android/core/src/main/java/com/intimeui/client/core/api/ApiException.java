package com.intimeui.client.core.api;

/** Error raised by the API client, carrying the server's business code. */
public final class ApiException extends Exception {
    public static final int NETWORK_ERROR = -1;
    public static final int INVALID_RESPONSE = -2;

    private final int code;
    private final int httpStatus;

    public ApiException(int code, int httpStatus, String message) {
        super(message);
        this.code = code;
        this.httpStatus = httpStatus;
    }

    public ApiException(int code, String message) {
        this(code, 0, message);
    }

    public int code() {
        return code;
    }

    public int httpStatus() {
        return httpStatus;
    }

    public boolean isNonceReplay() {
        return code == 40900;
    }

    public boolean isClockRejected() {
        return code == 40000;
    }

    public boolean isUnauthorized() {
        return code == 40100 || code == 40300;
    }

    public boolean isRetryable() {
        return code == NETWORK_ERROR || httpStatus >= 500 || httpStatus == 429;
    }

    public static ApiException network(String message, Throwable cause) {
        ApiException error = new ApiException(NETWORK_ERROR, 0, message);
        error.initCause(cause);
        return error;
    }

    public static ApiException invalidResponse(String message) {
        return new ApiException(INVALID_RESPONSE, 0, message);
    }
}
