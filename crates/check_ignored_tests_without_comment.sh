#!/usr/bin/env sh

grep -n -r '#\[ignore\]' ./rtlola-interpreter/src | grep -v '\/\/'
if [ "$?" -ne "1" ]; then
    echo "there are ignored test cases without accompanying comment"
    exit 1
fi
