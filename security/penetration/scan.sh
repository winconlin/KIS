#!/bin/bash
# KIS Penetration-Test — Automatisierter Scan
# Aufruf: ./scan.sh [BASE_URL] [ACCESS_TOKEN]

set -euo pipefail

BASE_URL="${1:-https://kis-test.local}"
ACCESS_TOKEN="${2:-}"
OUTPUT_DIR="reports/$(date +%Y%m%d_%H%M%S)"
DATE=$(date +%Y%m%d)

echo "=== KIS Penetration-Test Scan ==="
echo "Base URL: $BASE_URL"
echo "Output:   $OUTPUT_DIR"
echo ""

# Output-Verzeichnis erstellen
mkdir -p "$OUTPUT_DIR"

# 1. Nuclei-Scan
echo "[1/3] Nuclei-Scan..."
if command -v nuclei &> /dev/null; then
    nuclei -u "$BASE_URL" \
        -t nuclei-templates/kis/ \
        -H "Authorization: Bearer $ACCESS_TOKEN" \
        -o "$OUTPUT_DIR/nuclei_results.json" \
        -severity info,low,medium,high,critical \
        -rate-limit 100 \
        -timeout 10 \
        -retries 1
    echo "  ✓ Nuclei-Scan abgeschlossen"
else
    echo "  ✗ Nuclei nicht installiert"
fi

# 2. OWASP ZAP-Scan
echo "[2/3] OWASP ZAP-Scan..."
if command -v docker &> /dev/null; then
    docker run -it --rm \
        -v "$(pwd)/$OUTPUT_DIR:/zap/scan" \
        -e KIS_ACCESS_TOKEN="$ACCESS_TOKEN" \
        ghcr.io/zaproxy/zaproxy:stable \
        zap-baseline.py \
        -t "$BASE_URL" \
        -J /zap/scan/zap_results.json \
        -H "Authorization: Bearer $ACCESS_TOKEN" \
        -c 10 \
        -t 300
    echo "  ✓ ZAP-Scan abgeschlossen"
else
    echo "  ✗ Docker nicht installiert"
fi

# 3. Nikto-Scan (optional)
echo "[3/3] Nikto-Scan..."
if command -v nikto &> /dev/null; then
    nikto -h "$BASE_URL" \
        -o "$OUTPUT_DIR/nikto_results.html" \
        -Format htm
    echo "  ✓ Nikto-Scan abgeschlossen"
else
    echo "  ✗ Nikto nicht installiert (optional)"
fi

# Report erstellen
echo ""
echo "=== Report erstellen ==="
REPORT_TEMPLATE="../report-template.md"
if [ -f "$REPORT_TEMPLATE" ]; then
    cp "$REPORT_TEMPLATE" "$OUTPUT_DIR/report.md"
    echo "  ✓ Report-Vorlage kopiert"
    echo "  → Bearbeiten: $OUTPUT_DIR/report.md"
fi

# Zusammenfassung
echo ""
echo "=== Scan abgeschlossen ==="
echo "Ergebnisse in: $OUTPUT_DIR"
ls -lh "$OUTPUT_DIR"

echo ""
echo "Nächste Schritte:"
echo "1. $OUTPUT_DIR/report.md ausfüllen"
echo "2. Findings priorisieren (CVSS)"
echo "3. Maßnahmen vorschlagen"
echo "4. Report an Security-Team übergeben"
