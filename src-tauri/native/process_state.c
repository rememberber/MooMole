#include <sys/types.h>
#include <sys/sysctl.h>
#include <sys/proc.h>
#include <string.h>

/* proc_pidinfo/proc_pidpath do not return zombie records on Darwin.
 * Use SDK-defined kinfo_proc, never a hand-written Rust ABI or an ESRCH guess.
 * A short, denied, absent or mismatched response proves nothing. */
int moomole_process_is_zombie(int pid) {
    if (pid <= 0) return 0;
    int mib[] = {CTL_KERN, KERN_PROC, KERN_PROC_PID, pid};
    struct kinfo_proc info;
    memset(&info, 0, sizeof(info));
    size_t length = sizeof(info);
    if (sysctl(mib, 4, &info, &length, NULL, 0) != 0 ||
        length != sizeof(info) || info.kp_proc.p_pid != pid) return 0;
    return info.kp_proc.p_stat == SZOMB;
}
