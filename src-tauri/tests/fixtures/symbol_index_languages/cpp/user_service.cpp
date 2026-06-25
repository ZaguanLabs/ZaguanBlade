#include <string>
#include "user_service.hpp"

namespace Example::Users {

struct UserView {
    std::string id;
};

enum class UserStatus {
    Active,
    Disabled,
};

class UserService {
public:
    UserView find_user(const std::string& id) {
        return UserView{id};
    }
};

std::string normalize_user_id(const std::string& id) {
    return id;
}

} // namespace Example::Users
