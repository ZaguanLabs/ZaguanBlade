<?php

namespace App\Service;

use App\Repository\UserRepository;

final class UserService
{
    public function findUser(string $id): ?array
    {
        return [];
    }
}

interface UserFormatter
{
    public function formatUser(array $user): string;
}

trait LogsUsers
{
    protected function logUser(string $id): void
    {
    }
}

function normalize_user_id(string $id): string
{
    return trim($id);
}
