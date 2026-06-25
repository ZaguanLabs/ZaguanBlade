#include <stdbool.h>
#include "user_service.h"

struct user_view {
    const char *id;
};

enum user_status {
    USER_ACTIVE,
    USER_DISABLED,
};

struct user_view find_user(const char *id) {
    struct user_view view = { id };
    return view;
}

bool user_is_active(const struct user_view *user) {
    return user != 0;
}
