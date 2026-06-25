package com.example.users

import kotlinx.coroutines.Dispatchers
import com.example.shared.UserId as SharedUserId

class UserService {
    fun findUser(id: String): UserView {
        return UserView(id, "")
    }
}

interface UserFormatter {
    fun format(user: UserView): String
}

object UserCache

enum class UserStatus {
    Active,
    Disabled
}

data class UserView(val id: String, val name: String)

fun normalizeUserId(id: String): String = id.trim()
