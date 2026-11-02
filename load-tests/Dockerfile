# ============================================================
# KIS — k6-Image für Load-Testing (Task 5.6)
#
# Build:
#   docker build -f load-tests/Dockerfile -t kis-loadtest .
#
# Execution:
#   docker run --rm \
#     -v "$(pwd)/load-tests:/scripts" \
#     -e KIS_BASE_URL=http://host.docker.internal:8080 \
#     kis-loadtest run /scripts/patients.k6.js
#
# Das Image basiert auf k6io/k6:latest (offizielles k6-Image,
# enthält Go-Runtime + k6-Binary). Wir legen nur die Skripte
# hinein, damit der Run ohne -v-Mount möglich ist.
# ============================================================
FROM k6io/k6:latest

# k6-Skripte in das Image einbetten
COPY load-tests/patients.k6.js /scripts/patients.k6.js
COPY load-tests/appointments.k6.js /scripts/appointments.k6.js

WORKDIR /scripts

# k6 läuft als nicht-root User im Basis-Image
CMD ["k6", "run", "--help"]
