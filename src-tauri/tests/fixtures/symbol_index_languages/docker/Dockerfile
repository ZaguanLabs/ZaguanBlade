FROM node:22-alpine AS builder
WORKDIR /app
ARG APP_VERSION=dev
ENV NODE_ENV=production
COPY --from=builder /app/dist /usr/share/app
EXPOSE 8080
CMD ["node", "server.js"]
