namespace Example.Users;

using System.Collections.Generic;
using static System.String;
using Json = System.Text.Json.JsonSerializer;

public sealed class UserService
{
    public UserDto FindUser(string id)
    {
        return new UserDto(id, Empty);
    }
}

public interface IUserFormatter
{
    string Format(UserDto user);
}

public enum UserStatus
{
    Active,
    Disabled
}

public record UserDto(string Id, string Name);
