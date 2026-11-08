/* ---- Scull 0 ランタイム (scl_rt.h) ---- */
#include <inttypes.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/* 生成するすべての関数に付ける。使われない関数で警告を出さないようにする。 */
#define SCL_FN static __attribute__((unused))

/* 不変の文字列。Scull 0 ではメモリを解放しない。 */
typedef struct scl_str {
    const char *ptr;
    int64_t len;
} scl_str;

SCL_FN void scl_panic(const char *reason) __attribute__((noreturn));
SCL_FN void scl_panic(const char *reason) {
    fflush(stdout);
    fprintf(stderr, "scull: runtime error: %s\n", reason);
    exit(101);
}

/* ---- 文字列 ---- */

SCL_FN scl_str scl_str_make(const char *ptr, int64_t len) {
    scl_str s = { ptr, len };
    return s;
}

SCL_FN scl_str scl_str_concat(scl_str a, scl_str b) {
    int64_t len = a.len + b.len;
    char *p = malloc((size_t)len + 1);
    if (p == NULL) {
        scl_panic("out of memory");
    }
    memcpy(p, a.ptr, (size_t)a.len);
    memcpy(p + a.len, b.ptr, (size_t)b.len);
    p[len] = '\0';
    return scl_str_make(p, len);
}

SCL_FN scl_str scl_str_from_int(int64_t v) {
    char buf[32];
    int n = snprintf(buf, sizeof buf, "%" PRId64, v);
    char *p = malloc((size_t)n + 1);
    if (p == NULL) {
        scl_panic("out of memory");
    }
    memcpy(p, buf, (size_t)n + 1);
    return scl_str_make(p, n);
}

SCL_FN scl_str scl_str_from_bool(bool b) {
    return b ? scl_str_make("true", 4) : scl_str_make("false", 5);
}

SCL_FN bool scl_str_eq(scl_str a, scl_str b) {
    return a.len == b.len && memcmp(a.ptr, b.ptr, (size_t)a.len) == 0;
}

/* ---- 検査つき整数演算 ---- */

SCL_FN int64_t scl_add(int64_t a, int64_t b) {
    int64_t r;
    if (__builtin_add_overflow(a, b, &r)) {
        scl_panic("integer overflow");
    }
    return r;
}

SCL_FN int64_t scl_sub(int64_t a, int64_t b) {
    int64_t r;
    if (__builtin_sub_overflow(a, b, &r)) {
        scl_panic("integer overflow");
    }
    return r;
}

SCL_FN int64_t scl_mul(int64_t a, int64_t b) {
    int64_t r;
    if (__builtin_mul_overflow(a, b, &r)) {
        scl_panic("integer overflow");
    }
    return r;
}

SCL_FN int64_t scl_neg(int64_t a) {
    if (a == INT64_MIN) {
        scl_panic("integer overflow");
    }
    return -a;
}

SCL_FN int64_t scl_div(int64_t a, int64_t b) {
    if (b == 0) {
        scl_panic("division by zero");
    }
    if (a == INT64_MIN && b == -1) {
        scl_panic("integer overflow");
    }
    return a / b;
}

SCL_FN int64_t scl_rem(int64_t a, int64_t b) {
    if (b == 0) {
        scl_panic("remainder by zero");
    }
    if (b == -1) {
        return 0; /* INT64_MIN % -1 は C では未定義なので先に処理する */
    }
    return a % b;
}

/* ---- 組み込みエフェクト Console ---- */

typedef struct scl_eff_Console {
    void *env;
    void (*op_print)(void *env, scl_str s);
} scl_eff_Console;

SCL_FN void scl_rt_console_print(void *env, scl_str s) {
    (void)env;
    fwrite(s.ptr, 1, (size_t)s.len, stdout);
    fputc('\n', stdout);
}
/* ---- ランタイムここまで ---- */
