FROM ubuntu/squid:6.6-24.04_beta

COPY squid.conf.template /etc/squid/squid.conf.template
COPY docker-entrypoint.sh /docker-entrypoint-mount.sh

ENTRYPOINT [ "sh", "-c", "cp /docker-entrypoint-mount.sh /docker-entrypoint.sh && sed -i 's/\r$$//' /docker-entrypoint.sh && chmod +x /docker-entrypoint.sh && /docker-entrypoint.sh" ]
