/* SPDX-License-Identifier: GPL-2.0 */
/*
 * PetraOS vDSO (virtual Dynamic Shared Object) implementation.
 *
 * Provides userspace fast-path implementations for clock_gettime,
 * gettimeofday, time, clock_getres, getcpu, and rt_sigreturn.
 */

#define NULL ((void *)0)

typedef int clockid_t;
typedef long time_t;
typedef long suseconds_t;
typedef unsigned long uintptr_t;

#define CLOCK_REALTIME           0
#define CLOCK_MONOTONIC          1
#define CLOCK_PROCESS_CPUTIME_ID 2
#define CLOCK_THREAD_CPUTIME_ID  3
#define CLOCK_MONOTONIC_RAW      4
#define CLOCK_REALTIME_COARSE    5
#define CLOCK_MONOTONIC_COARSE   6
#define CLOCK_BOOTTIME           7
#define CLOCK_REALTIME_ALARM     8
#define CLOCK_BOOTTIME_ALARM     9
#define CLOCK_TAI                11

#define VCLOCK_NONE 0
#define VCLOCK_TSC  1

typedef unsigned char uint8_t;
typedef unsigned int uint32_t;
typedef unsigned long long uint64_t;
typedef long long int64_t;

struct timespec {
    int64_t tv_sec;
    int64_t tv_nsec;
};

struct timeval {
    int64_t tv_sec;
    int64_t tv_usec;
};

struct timezone {
    int tz_minuteswest;
    int tz_dsttime;
};

struct vdso_timestamp {
    uint64_t sec;
    uint64_t nsec;
};

struct vdso_data {
    uint32_t seq;
    uint32_t clock_mode;
    uint64_t cycle_last;
    uint64_t mask;
    uint32_t mult;
    uint32_t shift;
    struct vdso_timestamp basetime[4];
    struct vdso_timestamp coarse_realtime;
    struct vdso_timestamp coarse_monotonic;
};

/*
 * The vvar data page is placed exactly 4096 bytes below the vDSO base address.
 * Using RIP-relative addressing ensures pure position-independence without relocations.
 */
extern const char __ehdr_start;

static inline const struct vdso_data *get_vvar_data(void) {
    uintptr_t base;
    asm("lea __ehdr_start(%%rip), %0" : "=r"(base));
    return (const struct vdso_data *)(base - 4096);
}

static inline uint64_t read_tsc(void) {
    uint32_t lo, hi;
    asm volatile("lfence; rdtsc" : "=a"(lo), "=d"(hi) : : "memory");
    return ((uint64_t)hi << 32) | lo;
}

static inline long syscall2(long nr, long arg1, long arg2) {
    long ret;
    asm volatile(
        "syscall"
        : "=a"(ret)
        : "0"(nr), "D"(arg1), "S"(arg2)
        : "rcx", "r11", "memory"
    );
    return ret;
}

static inline long syscall3(long nr, long arg1, long arg2, long arg3) {
    long ret;
    asm volatile(
        "syscall"
        : "=a"(ret)
        : "0"(nr), "D"(arg1), "S"(arg2), "d"(arg3)
        : "rcx", "r11", "memory"
    );
    return ret;
}

static inline long fallback_clock_gettime(clockid_t clock_id, struct timespec *tp) {
    return syscall2(228 /* SYS_clock_gettime */, (long)clock_id, (long)tp);
}

static inline long fallback_gettimeofday(struct timeval *tv, struct timezone *tz) {
    return syscall2(96 /* SYS_gettimeofday */, (long)tv, (long)tz);
}

static inline long fallback_time(time_t *tloc) {
    return syscall2(201 /* SYS_time */, (long)tloc, 0);
}

static inline long fallback_clock_getres(clockid_t clock_id, struct timespec *res) {
    return syscall2(229 /* SYS_clock_getres */, (long)clock_id, (long)res);
}

static inline long fallback_getcpu(unsigned int *cpu, unsigned int *node, void *tcache) {
    return syscall3(309 /* SYS_getcpu */, (long)cpu, (long)node, (long)tcache);
}

__attribute__((visibility("default")))
int __vdso_clock_gettime(clockid_t clock_id, struct timespec *tp) {
    if (!tp) {
        return fallback_clock_gettime(clock_id, tp);
    }

    const struct vdso_data *vdata = get_vvar_data();

    /* Handle coarse clocks */
    if (clock_id == CLOCK_REALTIME_COARSE) {
        uint32_t seq;
        do {
            seq = __atomic_load_n(&vdata->seq, __ATOMIC_ACQUIRE);
            if (seq & 1) {
                asm volatile("pause");
                continue;
            }
            tp->tv_sec = vdata->coarse_realtime.sec;
            tp->tv_nsec = vdata->coarse_realtime.nsec;
        } while (__atomic_load_n(&vdata->seq, __ATOMIC_ACQUIRE) != seq);
        return 0;
    }

    if (clock_id == CLOCK_MONOTONIC_COARSE) {
        uint32_t seq;
        do {
            seq = __atomic_load_n(&vdata->seq, __ATOMIC_ACQUIRE);
            if (seq & 1) {
                asm volatile("pause");
                continue;
            }
            tp->tv_sec = vdata->coarse_monotonic.sec;
            tp->tv_nsec = vdata->coarse_monotonic.nsec;
        } while (__atomic_load_n(&vdata->seq, __ATOMIC_ACQUIRE) != seq);
        return 0;
    }

    /* Map clock_id to basetime index */
    int base_idx;
    switch (clock_id) {
        case CLOCK_REALTIME:
        case CLOCK_REALTIME_ALARM:
            base_idx = 0;
            break;
        case CLOCK_MONOTONIC:
        case CLOCK_MONOTONIC_RAW:
            base_idx = 1;
            break;
        case CLOCK_BOOTTIME:
        case CLOCK_BOOTTIME_ALARM:
            base_idx = 2;
            break;
        case CLOCK_TAI:
            base_idx = 3;
            break;
        default:
            return fallback_clock_gettime(clock_id, tp);
    }

    uint32_t seq;
    uint64_t sec = 0, nsec = 0;
    do {
        seq = __atomic_load_n(&vdata->seq, __ATOMIC_ACQUIRE);
        if (seq & 1) {
            asm volatile("pause");
            continue;
        }

        if (vdata->clock_mode != VCLOCK_TSC) {
            return fallback_clock_gettime(clock_id, tp);
        }

        uint64_t tsc = read_tsc();
        uint64_t delta = (tsc - vdata->cycle_last) & vdata->mask;
        uint64_t ns = (uint64_t)(((__uint128_t)delta * vdata->mult) >> vdata->shift);

        sec = vdata->basetime[base_idx].sec;
        nsec = vdata->basetime[base_idx].nsec + ns;
    } while (__atomic_load_n(&vdata->seq, __ATOMIC_ACQUIRE) != seq);

    tp->tv_sec = (int64_t)(sec + (nsec / 1000000000ULL));
    tp->tv_nsec = (int64_t)(nsec % 1000000000ULL);
    return 0;
}

int clock_gettime(clockid_t clock_id, struct timespec *tp)
    __attribute__((weak, alias("__vdso_clock_gettime")));

__attribute__((visibility("default")))
int __vdso_gettimeofday(struct timeval *tv, struct timezone *tz) {
    if (tv) {
        struct timespec ts;
        int ret = __vdso_clock_gettime(CLOCK_REALTIME, &ts);
        if (ret != 0) {
            return ret;
        }
        tv->tv_sec = ts.tv_sec;
        tv->tv_usec = (suseconds_t)(ts.tv_nsec / 1000);
    }
    if (tz) {
        tz->tz_minuteswest = 0;
        tz->tz_dsttime = 0;
    }
    return 0;
}

int gettimeofday(struct timeval *tv, struct timezone *tz)
    __attribute__((weak, alias("__vdso_gettimeofday")));

__attribute__((visibility("default")))
time_t __vdso_time(time_t *tloc) {
    struct timespec ts;
    if (__vdso_clock_gettime(CLOCK_REALTIME, &ts) == 0) {
        if (tloc) {
            *tloc = ts.tv_sec;
        }
        return ts.tv_sec;
    }
    return fallback_time(tloc);
}

time_t time(time_t *tloc)
    __attribute__((weak, alias("__vdso_time")));

__attribute__((visibility("default")))
int __vdso_clock_getres(clockid_t clock_id, struct timespec *res) {
    switch (clock_id) {
        case CLOCK_REALTIME:
        case CLOCK_MONOTONIC:
        case CLOCK_MONOTONIC_RAW:
        case CLOCK_BOOTTIME:
        case CLOCK_TAI:
            if (res) {
                res->tv_sec = 0;
                res->tv_nsec = 1;
            }
            return 0;
        case CLOCK_REALTIME_COARSE:
        case CLOCK_MONOTONIC_COARSE:
            if (res) {
                res->tv_sec = 0;
                res->tv_nsec = 1000000; /* 1 ms */
            }
            return 0;
        default:
            return fallback_clock_getres(clock_id, res);
    }
}

int clock_getres(clockid_t clock_id, struct timespec *res)
    __attribute__((weak, alias("__vdso_clock_getres")));

__attribute__((visibility("default")))
int __vdso_getcpu(unsigned int *cpu, unsigned int *node, void *tcache) {
    return fallback_getcpu(cpu, node, tcache);
}

int getcpu(unsigned int *cpu, unsigned int *node, void *tcache)
    __attribute__((weak, alias("__vdso_getcpu")));
