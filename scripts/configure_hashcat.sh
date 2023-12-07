# Configures the build for hashcat
# We disable unused modules for faster and smaller compilation target

set -e
CARGO_NAME=$(cargo run -- -V)
export PROJECT_NAME="${CARGO_NAME// /_}"
echo "Running command for $PROJECT_NAME"

cd hashcat
cd src
MODULES_DISABLE=""
for file in modules/*.c; do
  if [ "$file" != "modules/module_02000.c" ] && [ "$file" != "modules/module_00000.c" ] && [ "$file" != "modules/module_28510.c" ]; then
    MODULES_DISABLE="$MODULES_DISABLE ${file%.c}.dll ${file%.c}.so"
  fi
done
export ENABLE_UNRAR=0
export MODULES_DISABLE
cd ..