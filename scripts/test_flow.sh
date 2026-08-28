#!/usr/bin/env bash
# test_flow.sh - Prueba E2E rápida para inyectar y consultar un IOC en la API

set -e

API_URL=${API_URL:-"http://localhost:8080"}
TEST_DOMAIN="scam-test-$(date +%s).com"

echo "========================================================="
echo "  🚀 Iniciando Prueba E2E - Trampantojo MVP"
echo "  Dominio de prueba: $TEST_DOMAIN"
echo "========================================================="
echo ""

echo "1️⃣  Inyectando IOC ($TEST_DOMAIN) vía POST /v1/report..."
HTTP_STATUS=$(curl -s -o /dev/null -w "%{http_code}" -X POST "$API_URL/v1/report" \
  -H "Content-Type: application/json" \
  -d '{
    "indicator_type": "domain",
    "value": "'"$TEST_DOMAIN"'",
    "impersonates": "test_bank"
  }')

if [ "$HTTP_STATUS" -ne 202 ]; then
  echo "❌ Error: La API respondió con HTTP $HTTP_STATUS al inyectar el IOC."
  exit 1
fi
echo "✅ HTTP 202 Accepted. Reporte recibido."

echo ""
echo "⏳ Esperando 2 segundos para que el pipeline asíncrono (ClickHouse/Scoring) lo procese..."
sleep 2
echo ""

echo "2️⃣  Consultando el estado del IOC vía GET /v1/check..."
RESPONSE=$(curl -s "$API_URL/v1/check?value=$TEST_DOMAIN")

# Imprimir formateado si jq está disponible
if command -v jq >/dev/null 2>&1; then
  echo "$RESPONSE" | jq .
else
  echo "$RESPONSE"
fi

echo ""
if echo "$RESPONSE" | grep -q "\"value\":\"$TEST_DOMAIN\""; then
  echo "✅ E2E Exitoso: El IOC fue ingerido, guardado en Postgres y consultado correctamente."
else
  echo "❌ E2E Fallido: El IOC no se encontró en la respuesta del check."
  exit 1
fi
