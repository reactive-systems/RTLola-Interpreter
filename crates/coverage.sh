#!/bin/zsh

if [ "$1" = "--install" ]; then
  mkdir coverage
  cd coverage || exit

  echo "Installing grcov"
  git clone https://github.com/mozilla/grcov.git grcov_master
  cd grcov_master || exit
  cargo build --release
  mv ./target/release/grcov ../
  cd .. || exit
  rm -rf grcov_master

  rustup override set nightly

  echo "Installing llvm tools"
  rustup component add llvm-tools-preview

  echo "installing junit-xml"
  python3 -m pip install junit-xml

  echo "Setup done"
  exit
fi

export RUSTFLAGS="-Zinstrument-coverage"
export LLVM_PROFILE_FILE="coverage/coverage-%m.profraw"
cd coverage || exit

cargo test || true
python3 ../tests/rtlola-interpreter-e2e-tests.py || true
python3 ../tests/rtlola-interpreter-e2e-tests.py --online || true

./grcov ./ --binary-path ../target/debug/ -s ../ -t html --branch --ignore-not-existing --llvm -o output/


