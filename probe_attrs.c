// Independent C oracle for the APFS object and volume layouts used by tests.
// It is not linked into the product.
#include <errno.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <sys/attr.h>
#include <sys/mount.h>
#include <sys/stat.h>
#include <unistd.h>

struct buf {
    uint32_t length;
    attribute_set_t returned;
    off_t allocsize;
    off_t dataallocsize;
    off_t rsrcallocsize;
    off_t privatesize;
    uint64_t extflags;
} __attribute__((aligned(4), packed));

struct volume_buf {
    uint32_t length;
    attribute_set_t returned;
    off_t size;
    off_t free;
    off_t available;
    off_t used;
    vol_capabilities_attr_t capabilities;
    vol_attributes_attr_t attributes;
} __attribute__((aligned(4), packed));

static int print_volume(const char *path) {
    struct attrlist al;
    struct volume_buf b;
    struct statfs s;
    memset(&al, 0, sizeof(al));
    memset(&b, 0, sizeof(b));
    memset(&s, 0, sizeof(s));
    al.bitmapcount = ATTR_BIT_MAP_COUNT;
    al.commonattr = ATTR_CMN_RETURNED_ATTRS;
    al.volattr = ATTR_VOL_SIZE | ATTR_VOL_SPACEFREE | ATTR_VOL_SPACEAVAIL |
                 ATTR_VOL_SPACEUSED | ATTR_VOL_CAPABILITIES |
                 ATTR_VOL_ATTRIBUTES | ATTR_VOL_INFO;
    if (getattrlist(path, &al, &b, sizeof(b),
                    FSOPT_ATTR_CMN_EXTENDED | FSOPT_NOFOLLOW |
                        FSOPT_PACK_INVAL_ATTRS | FSOPT_RETURN_REALDEV) != 0) {
        fprintf(stderr, "volume getattrlist: %s\n", strerror(errno));
        return 1;
    }
    if (statfs(path, &s) != 0) {
        fprintf(stderr, "statfs: %s\n", strerror(errno));
        return 1;
    }
    printf("size=%lld free=%lld available=%lld used=%lld returned=0x%x valid=0x%x "
           "block_size=%u blocks=%llu blocks_free=%llu blocks_available=%llu\n",
           (long long)b.size, (long long)b.free, (long long)b.available,
           (long long)b.used, b.returned.volattr,
           b.attributes.validattr.volattr, s.f_bsize,
           (unsigned long long)s.f_blocks, (unsigned long long)s.f_bfree,
           (unsigned long long)s.f_bavail);
    return 0;
}

int main(int argc, char **argv) {
    if (argc == 3 && strcmp(argv[1], "--volume") == 0) {
        return print_volume(argv[2]);
    }
    for (int i = 1; i < argc; i++) {
        struct attrlist al;
        struct buf b;
        memset(&al, 0, sizeof(al));
        memset(&b, 0, sizeof(b));

        al.bitmapcount = ATTR_BIT_MAP_COUNT;
        al.commonattr = ATTR_CMN_RETURNED_ATTRS;
        al.fileattr = ATTR_FILE_ALLOCSIZE | ATTR_FILE_DATAALLOCSIZE |
                      ATTR_FILE_RSRCALLOCSIZE;
        al.forkattr = ATTR_CMNEXT_PRIVATESIZE | ATTR_CMNEXT_EXT_FLAGS;

        // PACK_INVAL_ATTRS keeps the buffer layout fixed when an attribute is
        // not returned; without it a missing attribute shifts every later field.
        if (getattrlist(argv[i], &al, &b, sizeof(b),
                        FSOPT_ATTR_CMN_EXTENDED | FSOPT_NOFOLLOW |
                            FSOPT_PACK_INVAL_ATTRS) != 0) {
            printf("%-46s ERR %s\n", argv[i], strerror(errno));
            continue;
        }

        struct stat st;
        long long blocks = (lstat(argv[i], &st) == 0) ? (long long)st.st_blocks * 512 : -1;

        printf("%-46s alloc=%-10lld data=%-10lld rsrc=%-9lld private=%-10lld "
               "extflags=0x%llx st_blocks=%lld%s%s\n",
               argv[i], (long long)b.allocsize, (long long)b.dataallocsize,
               (long long)b.rsrcallocsize, (long long)b.privatesize,
               (unsigned long long)b.extflags, blocks,
               (b.returned.forkattr & ATTR_CMNEXT_PRIVATESIZE) ? "" : " [private UNMEASURABLE]",
               (st.st_flags & UF_COMPRESSED) ? " [UF_COMPRESSED]" : "");

        if ((b.returned.fileattr & al.fileattr) != al.fileattr ||
            (b.returned.forkattr & al.forkattr) != al.forkattr) {
            printf("%-46s WARN incomplete returned mask: file=0x%x fork=0x%x\n",
                   argv[i], b.returned.fileattr, b.returned.forkattr);
        }
    }
    return 0;
}
