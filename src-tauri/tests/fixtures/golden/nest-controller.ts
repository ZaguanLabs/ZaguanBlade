import { Controller, Get } from "@nestjs/common";

@Controller("users")
export class UsersController {
    @Get(":id")
    findOne(id: string): string {
        return id;
    }
}
