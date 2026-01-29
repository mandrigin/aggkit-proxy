FROM nginx:alpine

COPY index.html style.css app.js /usr/share/nginx/html/

# Inject bridge address at container start so the UI can read it.
# The entrypoint replaces a placeholder in index.html with the env value.
COPY docker-entrypoint.sh /docker-entrypoint.d/40-inject-config.sh
RUN chmod +x /docker-entrypoint.d/40-inject-config.sh

EXPOSE 80
