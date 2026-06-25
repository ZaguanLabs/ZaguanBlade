package com.example.users;

import java.util.Optional;
import static java.util.Collections.emptyList;

public final class UserService {
    public Optional<UserView> findUser(String id) {
        return Optional.empty();
    }
}

interface UserFormatter {
    String format(UserView user);
}

enum UserStatus {
    ACTIVE,
    DISABLED
}

record UserView(String id, String name) {
}
