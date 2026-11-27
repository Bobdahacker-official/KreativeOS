#include "kos.h"

#include <crypt.h>
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <grp.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/random.h>
#include <sys/stat.h>
#include <unistd.h>

#define ACCOUNT_LIMIT 16
#define FIRST_UID 1000
#define INPUT_GID 108
#define ACCOUNT_DB "/data/system/accounts"

typedef struct { char name[33], hash[CRYPT_OUTPUT_SIZE]; unsigned uid; } Account;

static bool valid_username(const char *s)
{
    size_t n = strlen(s);
    if (!n || n > 32 || *s < 'a' || *s > 'z') return false;
    for (; *s; s++)
        if (!(*s >= 'a' && *s <= 'z') && !(*s >= '0' && *s <= '9') && *s != '_' && *s != '-') return false;
    return strcmp(s - n, "root") && strcmp(s - n, "nobody");
}

static int read_accounts(Account list[ACCOUNT_LIMIT])
{
    FILE *f = fopen(ACCOUNT_DB, "re");
    if (!f) return errno == ENOENT ? 0 : -1;
    char line[512];
    int count = 0;
    while (fgets(line, sizeof(line), f)) {
        Account a;
        char end;
        if (count == ACCOUNT_LIMIT ||
            sscanf(line, "%32[a-z0-9_-]:%u:%383[^:\n]%c", a.name, &a.uid, a.hash, &end) != 4 ||
            end != '\n' || !valid_username(a.name) || a.uid != (unsigned)(FIRST_UID + count) ||
            strncmp(a.hash, "$y$", 3)) {
            count = -1;
            break;
        }
        list[count++] = a;
    }
    if (ferror(f)) count = -1;
    fclose(f);
    return count;
}

static bool finish_file(FILE *file, const char *temp, const char *path, bool good)
{
    if (fflush(file) || fsync(fileno(file))) good = false;
    if (fclose(file)) good = false;
    if (good && rename(temp, path) == 0) return true;
    unlink(temp);
    return false;
}

static bool export_accounts(const Account *list, int count)
{
    FILE *passwd = fopen("/etc/passwd.new", "we");
    if (!passwd) return false;
    FILE *shadow = fopen("/etc/shadow.new", "we");
    if (!shadow) {
        fclose(passwd);
        unlink("/etc/passwd.new");
        return false;
    }
    fchmod(fileno(passwd), 0644);
    fchmod(fileno(shadow), 0600);
    fputs("root:x:0:0:root:/root:/bin/sh\nnobody:x:65534:65534:nobody:/nonexistent:/bin/false\n", passwd);
    fputs("root:!:0:0:99999:7:::\nnobody:!:0:0:99999:7:::\n", shadow);
    for (int i = 0; i < count; i++) {
        const Account *a = &list[i];
        fprintf(passwd, "%s:x:%u:%u:%s:/data/home/%s:/bin/sh\n", a->name, a->uid, a->uid, a->name, a->name);
        fprintf(shadow, "%s:%s:0:0:99999:7:::\n", a->name, a->hash);
    }
    bool ok = finish_file(shadow, "/etc/shadow.new", "/etc/shadow", !ferror(shadow));
    return finish_file(passwd, "/etc/passwd.new", "/etc/passwd", ok && !ferror(passwd));
}

static bool system_directory(const char *path, mode_t mode)
{
    if (mkdir(path, mode) && errno != EEXIST) return false;
    int fd = open(path, O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC);
    if (fd < 0) return false;
    struct stat st;
    bool safe = fstat(fd, &st) == 0 && st.st_uid == 0 && fchmod(fd, mode) == 0;
    close(fd);
    return safe;
}

bool account_bootstrap(void)
{
    if (geteuid() != 0) return false;
    umask(077);
    if (chown("/data", 0, 0) || chmod("/data", 0755)) return false;
    if (!system_directory("/data/system", 0700) || !system_directory("/data/home", 0755)) return false;
    Account list[ACCOUNT_LIMIT];
    int count = read_accounts(list);
    return count >= 0 && export_accounts(list, count);
}

static bool same_secret(const char *a, const char *b)
{
    size_t x = strlen(a), y = strlen(b);
    if (x != y) return false;
    unsigned different = 0;
    for (size_t i = 0; i < x; i++) different |= (unsigned char)a[i] ^ (unsigned char)b[i];
    return different == 0;
}

static bool authenticate(const char *password, const char *hash)
{
    struct crypt_data data = {0};
    char *computed = crypt_r(password, hash, &data);
    bool good = computed && same_secret(computed, hash);
    explicit_bzero(&data, sizeof(data));
    return good;
}

static bool hash_password(const char *password, char hash[CRYPT_OUTPUT_SIZE])
{
    unsigned char bytes[32];
    char salt[CRYPT_GENSALT_OUTPUT_SIZE];
    struct crypt_data data = {0};
    if (getrandom(bytes, sizeof(bytes), 0) != sizeof(bytes) ||
        !crypt_gensalt_rn("$y$", 0, (char *)bytes, sizeof(bytes), salt, sizeof(salt))) return false;
    const char *value = crypt_r(password, salt, &data);
    bool good = value && *value == '$';
    if (good) strcpy(hash, value);
    explicit_bzero(&data, sizeof(data));
    explicit_bzero(bytes, sizeof(bytes));
    return good;
}

static bool write_accounts(const Account *list, int count)
{
    char temporary[] = "/data/system/.accounts-XXXXXX";
    int fd = mkostemp(temporary, O_CLOEXEC);
    if (fd < 0) return false;
    FILE *file = fdopen(fd, "w");
    if (!file) {
        close(fd);
        unlink(temporary);
        return false;
    }
    for (int i = 0; i < count; i++)
        fprintf(file, "%s:%u:%s\n", list[i].name, list[i].uid, list[i].hash);
    if (!finish_file(file, temporary, ACCOUNT_DB, !ferror(file))) return false;
    int dir = open("/data/system", O_RDONLY | O_DIRECTORY | O_CLOEXEC);
    bool saved = dir >= 0 && fsync(dir) == 0;
    if (dir >= 0) close(dir);
    return saved;
}

static void import_legacy(int home, unsigned uid)
{
    DIR *source = opendir("/data");
    if (!source) return;
    struct dirent *entry;
    while ((entry = readdir(source))) {
        size_t len = strlen(entry->d_name);
        if (len < 5 || strcmp(entry->d_name + len - 4, ".txt")) continue;
        int old = openat(dirfd(source), entry->d_name, O_RDONLY | O_CLOEXEC | O_NOFOLLOW | O_NONBLOCK);
        if (old < 0) continue;
        struct stat st;
        if (fstat(old, &st) == 0 && S_ISREG(st.st_mode) && st.st_size >= 0 && st.st_size <= NOTE_CAP) {
            int dest = openat(home, entry->d_name, O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW | O_CLOEXEC, 0600);
            if (dest >= 0) {
                char bytes[4096];
                ssize_t count;
                bool copied = true;
                while ((count = read(old, bytes, sizeof(bytes))) > 0)
                    if (write(dest, bytes, (size_t)count) != count) { copied = false; break; }
                if (count < 0 || fchown(dest, uid, uid) || fsync(dest)) copied = false;
                close(dest);
                if (!copied) unlinkat(home, entry->d_name, 0);
            }
        }
        close(old);
    }
    closedir(source);
}

static bool add_account(Account *list, int index, Login *l)
{
    Account *a = &list[index];
    strcpy(a->name, l->user);
    if (!hash_password(l->password, a->hash)) return false;
    a->uid = FIRST_UID + index;
    char path[64];
    snprintf(path, sizeof(path), "/data/home/%s", a->name);
    if (mkdir(path, 0700) && errno != EEXIST) return false;
    int home = open(path, O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC);
    if (home < 0) return false;
    bool ok = fchown(home, a->uid, a->uid) == 0 && fchmod(home, 0700) == 0;
    if (ok && index == 0) import_legacy(home, a->uid);
    if (ok) ok = fsync(home) == 0;
    close(home);
    return ok && write_accounts(list, index + 1) && export_accounts(list, index + 1);
}

void account_submit(State *s)
{
    Login *l = &s->login;
    if (l->retry_at && (int)(s->now - l->retry_at) < 0) return;
    Account list[ACCOUNT_LIMIT];
    int count = read_accounts(list);
    if (count < 0) { strcpy(s->message, "The account store could not be read."); return; }
    int found = -1;
    for (int i = 0; i < count; i++) if (!strcmp(list[i].name, l->user)) found = i;
    if (l->setup || l->add) {
        if (!valid_username(l->user)) { strcpy(s->message, "Use a lowercase username, numbers, _ or -."); return; }
        if (l->add && (!count || !authenticate(l->owner_password, list[0].hash))) {
            strcpy(s->message, "The owner password is incorrect.");
            explicit_bzero(l->owner_password, sizeof(l->owner_password));
            l->retry_at = s->now + 1500;
            return;
        }
        if (found >= 0) { strcpy(s->message, "That username is already taken."); return; }
        if (count >= ACCOUNT_LIMIT) { strcpy(s->message, "This device has reached its account limit."); return; }
        if (strlen(l->password) < 8) { strcpy(s->message, "Choose a password with at least 8 characters."); return; }
        if (!same_secret(l->password, l->confirmation)) { strcpy(s->message, "The passwords do not match."); return; }
        if (!add_account(list, count, l)) { strcpy(s->message, "Could not save the account to storage."); return; }
        found = count;
    } else if (found < 0 || !authenticate(l->password, list[found].hash)) {
        strcpy(s->message, "Username or password is incorrect.");
        explicit_bzero(l->password, sizeof(l->password));
        l->retry_at = s->now + 1500;
        return;
    }
    l->uid = list[found].uid;
    l->gid = list[found].uid;
    snprintf(l->home, sizeof(l->home), "/data/home/%s", list[found].name);
    l->done = true;
    explicit_bzero(l->password, sizeof(l->password));
    explicit_bzero(l->confirmation, sizeof(l->confirmation));
    explicit_bzero(l->owner_password, sizeof(l->owner_password));
}

void account_key(State *s, int key)
{
    Login *l = &s->login;
    char *fields[] = { l->user, l->password, l->confirmation, l->owner_password };
    size_t sizes[] = { sizeof(l->user), sizeof(l->password), sizeof(l->confirmation), sizeof(l->owner_password) };
    int last = l->add ? 3 : l->setup ? 2 : 1;
    if (key == '\t' || (key == '\n' && l->field < last)) {
        l->field = (l->field + 1) % (last + 1);
        return;
    }
    if (key == '\n') { account_submit(s); return; }
    size_t len = strlen(fields[l->field]);
    if (key == '\b' && len) fields[l->field][len - 1] = 0;
    if (key >= 32 && key <= 126 && len + 1 < sizes[l->field]) {
        fields[l->field][len] = (char)key;
        fields[l->field][len + 1] = 0;
    }
}

bool account_enter(State *s)
{
    const Login *l = &s->login;
    if (!l->done || l->uid < FIRST_UID) return false;
    int group = open("/sys/fs/cgroup/kreative-session/cgroup.procs", O_WRONLY | O_CLOEXEC);
    if (group >= 0) {
        bool moved = write(group, "0", 1) == 1;
        close(group);
        if (!moved) return false;
    }
    gid_t groups[] = { l->gid, INPUT_GID };
    if (setgroups(ARRAY_LEN(groups), groups) || setgid(l->gid) || setuid(l->uid)) return false;
    if (chdir(l->home)) return false;
    umask(077);
    return true;
}

bool account_first_run(void)
{
    Account list[ACCOUNT_LIMIT];
    return read_accounts(list) == 0;
}

void account_cleanup(uid_t uid)
{
    if (uid < FIRST_UID || uid >= FIRST_UID + ACCOUNT_LIMIT) return;
    int group = open("/sys/fs/cgroup/kreative-session/cgroup.kill", O_WRONLY | O_CLOEXEC);
    if (group >= 0) {
        bool killed = write(group, "1", 1) == 1;
        close(group);
        if (killed) return;
    }
    DIR *proc = opendir("/proc");
    if (!proc) return;
    struct dirent *entry;
    while ((entry = readdir(proc))) {
        char *end;
        long pid = strtol(entry->d_name, &end, 10);
        if (pid <= 1 || *end) continue;
        struct stat st;
        if (!fstatat(dirfd(proc), entry->d_name, &st, AT_SYMLINK_NOFOLLOW) &&
            st.st_uid == uid) kill((pid_t)pid, SIGKILL);
    }
    closedir(proc);
}
