# DuckDB bundled libraries

Covered `duckdb` version: `1.10505.0` (DuckDB 1.5.5)

Covered libduckdb-sys version: `1.10505.0`

Source: `libduckdb-sys-1.10505.0/duckdb.tar.gz`, `third_party/`

This notice covers the C and C++ libraries compiled into the bundled DuckDB
engine. The published tarball contains no separate `LICENSE`, `COPYING`, or
`NOTICE` files for these directories, so the attributions below are derived
from the vendored source headers. The source archive must be re-reviewed when
the pinned `libduckdb-sys` version changes.

## DuckDB

- License: MIT
- Copyright: 2021-2026 Stichting DuckDB Foundation
- Source: `libduckdb-sys/LICENSE`

## fastpforlib

- License: Apache-2.0
- Copyright: Daniel Lemire
- Source header: `third_party/fastpforlib/bitpacking.h`

The source header states: “This code is released under the Apache License
Version 2.0 http://www.apache.org/licenses/.”

## fmt

- License: MIT, with the source-header exception for embedded object code
- Copyright: 2012-present Victor Zverovich
- Source header: `third_party/fmt/include/fmt/format.h`

The source header includes the MIT permission, warranty disclaimer, and this
exception: portions embedded in machine-executable object form may be
redistributed without repeating the copyright and permission notices.

## fsst

- License: MIT
- Copyright: 2018-2020 CWI, TU Munich, FSU Jena
- Source header: `third_party/fsst/libfsst.cpp`

The source header includes the MIT permission, warranty disclaimer, and the
requirement to retain the copyright and permission notices.

## hyperloglog

- License: BSD-3-Clause
- Copyright: 2014 Salvatore Sanfilippo
- Source header: `third_party/hyperloglog/hyperloglog.cpp`

The source header includes the BSD-3-Clause redistribution conditions and
disclaimer for the Redis HyperLogLog implementation.

## libpg_query

- License: PostgreSQL License (BSD-style), plus GNU Bison 2.3's GPL-2.0-or-later
  parser-skeleton exception
- Copyright: PostgreSQL Global Development Group; Regents of the University of
  California; Free Software Foundation, Inc.
- Source headers: `third_party/libpg_query/src_backend_nodes_list.cpp`,
  `third_party/libpg_query/src_backend_parser_scan.cpp`, and
  `third_party/libpg_query/src_backend_parser_gram.cpp`

The PostgreSQL source headers carry these notices:

```text
Portions Copyright (c) 1996-2017, PostgreSQL Global Development PGGroup
Portions Copyright (c) 1994, Regents of the University of California
```

The generated Bison skeleton carries the GNU General Public License, version 2
or later, and the following special exception:

```text
As a special exception, you may create a larger work that contains part or all
of the Bison parser skeleton and distribute that work under terms of your
choice, so long as that work isn't itself a parser generator using the skeleton
or a modified version thereof as a parser skeleton.
```

## mbedtls

- License: Apache-2.0 OR GPL-2.0-or-later
- Copyright: The Mbed TLS Contributors
- Source headers: `third_party/mbedtls/include/mbedtls/cipher.h` and the
  compiled files under `third_party/mbedtls/library/`

The vendored headers identify the terms with:

```text
Copyright The Mbed TLS Contributors
SPDX-License-Identifier: Apache-2.0 OR GPL-2.0-or-later
```

## miniz

- License: MIT for the compiled implementation; the miniz header also
  identifies the deflate/inflate implementation as public domain under the
  Unlicense
- Copyright: 2010-2014 Rich Geldreich and Tenacious Software LLC; 2013-2014
  RAD Game Tools and Valve Software
- Source headers: `third_party/miniz/miniz.cpp` and
  `third_party/miniz/miniz.hpp`

The implementation header begins with “public domain” and refers to its
Unlicense statement. The compiled source carries the MIT-style permission and
disclaimer with the copyright lines above.

## re2

- License: BSD-3-Clause, with additional Unicode-data terms
- Copyright: The RE2 Authors; 2002 Lucent Technologies for the UTF helper
- Source headers: `third_party/re2/re2/re2.h` and
  `third_party/re2/util/utf.h`

The RE2 headers state that use is governed by the BSD-style license. The UTF
helper additionally carries this notice:

```text
The authors of this software are Rob Pike and Ken Thompson.
Copyright (c) 2002 by Lucent Technologies.
Permission to use, copy, modify, and distribute this software for any purpose
without fee is hereby granted, provided that this entire notice is included in
all copies of any software which is or includes a copy or modification of this
software and in all copies of the supporting documentation for such software.
```

## skiplist

- License: MIT
- Copyright: 2015-2023 Paul Ross
- Source header: `third_party/skiplist/SkipList.h`

The source header includes the MIT permission and warranty disclaimer.

## utf8proc

- License: MIT, with Unicode data attribution
- Copyright: 2014-2021 Steven G. Johnson, Jiahao Chen, Peter Colberg, Tony
  Kelman, Scott P. Jones, and other contributors; 2009 Public Software Group
  e. V., Berlin, Germany
- Source header: `third_party/utf8proc/utf8proc.cpp`

The source header includes the MIT permission and disclaimer. It also states
that the library contains derived data from modified Unicode data files and
points to <https://www.unicode.org/Public/UNIDATA/> and the data-file copyright
statement.

## yyjson

- License: MIT
- Copyright: 2020 YaoYuan
- Source header: `third_party/yyjson/yyjson.cpp`

The source header includes the MIT permission and warranty disclaimer.

## zstd

- License: BSD-3-Clause OR GPL-2.0, at the user's choice
- Copyright: Meta Platforms, Inc. and affiliates; additional zstd contributors
- Source header: `third_party/zstd/common/zstd_common.cpp`

The source header states:

```text
This source code is licensed under both the BSD-style license (found in the
LICENSE file in the root directory of this source tree) and the GPLv2 (found
in the COPYING file in the root directory of this source tree).
You may select, at your option, one of the above-listed licenses.
```

## Common MIT license text

The MIT-licensed components above use the following permission and disclaimer
text as shown in their vendored headers:

```text
Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```
