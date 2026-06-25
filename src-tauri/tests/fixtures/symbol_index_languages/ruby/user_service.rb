require "json"
require_relative "user_formatter"

module Example::Users
  class UserService
    def find_user(id)
      { id: id }
    end

    def self.normalize_user_id(id)
      id.strip
    end

    def active?
      true
    end
  end
end

def format_user(user)
  JSON.generate(user)
end
