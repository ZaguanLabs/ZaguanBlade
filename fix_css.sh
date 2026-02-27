#!/bin/bash
head -n 186 src/index.css > src/index.css.tmp
echo "}" >> src/index.css.tmp
mv src/index.css.tmp src/index.css
