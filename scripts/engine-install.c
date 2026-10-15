// Development build utility, never bundled or invoked by the application.
#include <fcntl.h>
#include <stdio.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>
#include <errno.h>

int main(int argc, char **argv) {
    if (argc != 3 || strncmp(argv[2], ".mole-stage-", 12) != 0 || strchr(argv[2], '/')) return 2;
    int parent = open(argv[1], O_RDONLY | O_DIRECTORY | O_NOFOLLOW);
    if (parent < 0) { perror("engine parent"); return 1; }
    struct stat stage, old;
    int result = -1;
    if (fstatat(parent, argv[2], &stage, AT_SYMLINK_NOFOLLOW) != 0) goto done;
    if (!S_ISDIR(stage.st_mode)) { errno = EINVAL; goto done; }
    if (fstatat(parent, "mole", &old, AT_SYMLINK_NOFOLLOW) != 0) {
        if (errno == ENOENT) result = renameatx_np(parent, argv[2], parent, "mole", RENAME_EXCL);
    } else if (S_ISDIR(old.st_mode)) {
        result = renameatx_np(parent, argv[2], parent, "mole", RENAME_SWAP);
    } else errno = EINVAL;
done:
    if (result != 0) perror("engine publication refused");
    close(parent);
    return result == 0 ? 0 : 1;
}
