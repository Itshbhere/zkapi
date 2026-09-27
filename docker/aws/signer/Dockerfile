FROM node:24-bookworm-slim
WORKDIR /app
COPY package.json package-lock.json ./
RUN npm ci --omit=dev --ignore-scripts && npm cache clean --force
COPY signer.mjs server.mjs ./
USER node
ENV NODE_ENV=production
EXPOSE 8547
CMD ["node", "server.mjs"]
