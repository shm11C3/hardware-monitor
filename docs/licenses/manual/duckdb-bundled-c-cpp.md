## DuckDB bundled C/C++ libraries (via libduckdb-sys)

- License: see the per-library table below
- Repository: [duckdb/duckdb-rs](https://github.com/duckdb/duckdb-rs)
- Source: `libduckdb-sys` crate, `duckdb.tar.gz`

Covered libduckdb-sys version: 1.10505.0
Covered duckdb version: 1.10505.0
Covered duckdb features: default-features = false, features = ["bundled"]

Builds with the `duckdb-archive` Cargo feature statically link DuckDB from the
`duckdb.tar.gz` archive vendored inside the `libduckdb-sys` crate. That archive
carries no `LICENSE`, `COPYING` or `NOTICE` files, so `cargo license` and
`cargo-deny` see only the crate-level license of `libduckdb-sys` itself (MIT)
and nothing about the third-party C/C++ code compiled into the binary. This
entry is the attribution for that code and is maintained by hand.

Scope: the crate is used with `default-features = false, features = ["bundled"]`
(`core/Cargo.toml`), so the `cc` build backend compiles the `base` and
`core_functions` sections of `duckdb/manifest.json` only. The `parquet` and
`json` extensions are not built, so `brotli`, `lz4`, `snappy`, `thrift` and the
`parquet` extension sources present in the archive do **not** ship and are not
listed here. Enabling another `duckdb` crate feature changes that set, which is
why the feature configuration is recorded above and checked in CI. Every
license below has a GPL-3.0-compatible arm, so none of them conflicts with the
application license (`docs/adr/0020-relicense-to-gpl-3.0-or-later.md`).

### How to refresh this entry

1. Bump `duckdb` in `core/Cargo.toml` and update `Cargo.lock`.
2. Extract the archive that ships with the new crate version:
   `tar -xzf ~/.cargo/registry/src/*/libduckdb-sys-<version>/duckdb.tar.gz -C <tmp>`
3. Re-derive the compiled set from `duckdb/manifest.json` (`base.cpp_files` /
   `base.include_dirs` plus the sections for any enabled extension feature),
   and confirm which `third_party/` directories are reached.
4. Read the license header of each library from the extracted sources and
   update the table and notices below.
5. Update the three "Covered ..." lines above.
   `.github/scripts/check-duckdb-notice.ts` compares them with `Cargo.lock` and
   `core/Cargo.toml` and fails CI if they drift.

### Libraries

Paths are relative to the root of `duckdb.tar.gz`.

| Library | License (SPDX) | Copyright | License text read from |
| --- | --- | --- | --- |
| fastpforlib | Apache-2.0 | (c) Daniel Lemire | `duckdb/third_party/fastpforlib/bitpacking.h` |
| fmt | MIT (with the fmt object-form exception) | Copyright (c) 2012 - present, Victor Zverovich | `duckdb/third_party/fmt/include/fmt/format.h` |
| fsst | MIT | Copyright 2018-2020, CWI, TU Munich, FSU Jena | `duckdb/third_party/fsst/libfsst.hpp` |
| hyperloglog | BSD-3-Clause | Copyright (c) 2014, Salvatore Sanfilippo; Copyright (c) 2006-2015, Salvatore Sanfilippo; Copyright (c) 2015, Oran Agra; Copyright (c) 2015, Redis Labs, Inc | `duckdb/third_party/hyperloglog/hyperloglog.cpp`, `duckdb/third_party/hyperloglog/sds.hpp` |
| libpg_query | PostgreSQL; the generated parser additionally GPL-2.0-or-later WITH Bison-exception-2.2 | Portions Copyright (c) 1996-2018, PostgreSQL Global Development Group; Portions Copyright (c) 1994, Regents of the University of California; Copyright (C) 1984-2006 Free Software Foundation, Inc. (Bison skeleton) | `duckdb/third_party/libpg_query/src_common_keywords.cpp`, `duckdb/third_party/libpg_query/src_backend_parser_gram.cpp` |
| mbedtls | Apache-2.0 OR GPL-2.0-or-later (taken under Apache-2.0) | Copyright The Mbed TLS Contributors | `duckdb/third_party/mbedtls/library/sha256.cpp` |
| miniz | Unlicense; the ZIP layer additionally MIT | Copyright 2013-2014 RAD Game Tools and Valve Software; Copyright 2010-2014 Rich Geldreich and Tenacious Software LLC | `duckdb/third_party/miniz/miniz.cpp` |
| re2 | BSD-3-Clause; `util/rune.cc` and `util/utf.h` under the Lucent/Plan 9 permissive notice | Copyright 1999-2023 The RE2 Authors; Copyright (c) 2002 by Lucent Technologies | `duckdb/third_party/re2/re2/re2.h`, `duckdb/third_party/re2/util/rune.cc` |
| skiplist | MIT | Copyright (c) 2015-2023 Paul Ross. All rights reserved. (file header); Copyright (c) 2017-2023 Paul Ross (MIT block in the same file); Copyright (c) 2017 Paul Ross. All rights reserved. (`NodeRefs.h`, `SkipList.cpp`) | `duckdb/third_party/skiplist/SkipList.h` |
| utf8proc | MIT | Copyright (c) 2014-2021 Steven G. Johnson, Jiahao Chen, Peter Colberg, Tony Kelman, Scott P. Jones, and other contributors; Copyright (c) 2009 Public Software Group e. V., Berlin, Germany | `duckdb/third_party/utf8proc/utf8proc.cpp` |
| yyjson | MIT | Copyright (c) 2020 YaoYuan | `duckdb/third_party/yyjson/yyjson.cpp` |
| zstd | BSD-3-Clause OR GPL-2.0 (taken under BSD-3-Clause) | Copyright (c) Meta Platforms, Inc. and affiliates | `duckdb/third_party/zstd/compress/zstd_compress.cpp` |
| concurrentqueue | BSD-2-Clause | Copyright (c) 2013-2016, Cameron Desrochers | `duckdb/third_party/concurrentqueue/concurrentqueue.h` |
| fast_float | MIT | Daniel Lemire, João Paulo Magalhaes and contributors (the vendored copy states no copyright line) | `duckdb/third_party/fast_float/fast_float/fast_float.h` |
| httplib | MIT | Copyright (c) 2025 Yuji Hirose | `duckdb/third_party/httplib/httplib.hpp` |
| jaro_winkler | MIT | Copyright (c) 2022 Max Bachmann | `duckdb/third_party/jaro_winkler/jaro_winkler.hpp` |
| pcg | Apache-2.0 OR MIT (taken under MIT) | Copyright 2014-2019 Melissa O'Neill and the PCG Project contributors | `duckdb/third_party/pcg/pcg_random.hpp` |
| pdqsort | Zlib | Copyright (c) 2021 Orson Peters | `duckdb/third_party/pdqsort/pdqsort.h` |
| ska_sort | BSL-1.0 | Copyright Malte Skarupke 2016 | `duckdb/third_party/ska_sort/ska_sort.hpp` |
| tdigest | Apache-2.0 | Licensed to Derrick R. Burns under one or more contributor license agreements | `duckdb/third_party/tdigest/t_digest.hpp` |
| vergesort | MIT | Copyright (c) 2015-2020 Morwenn | `duckdb/third_party/vergesort/vergesort.h` |

The last nine are header-only libraries. They carry no `.cpp` file of their own
in the manifest, but DuckDB core sources that are compiled into the binary
include their headers (for example `src/parallel/task_scheduler.cpp` includes
`concurrentqueue.h`, and the always-enabled `core_functions` extension source
`approximate_quantile.cpp` includes `t_digest.hpp`), so their code does ship.

## License texts, as vendored in duckdb.tar.gz

The archive carries license text only as source-file header comments. Every
block in this section is reproduced verbatim from the file named above it.
Terms that the archive references but does not carry are reproduced in the
following section instead.

MIT, from `duckdb/third_party/skiplist/SkipList.h`. Applies to fmt, fsst,
skiplist, utf8proc, yyjson, fast_float, httplib, jaro_winkler, pcg and
vergesort, each under its own copyright line from the table above. The block is
quoted verbatim, so it carries skiplist's own `2017-2023` MIT copyright line;
that is a separate notice from the `2015-2023` file header above it in the same
file, and both are listed for skiplist in the table:

```LICENSE
MIT License

Copyright (c) 2017-2023 Paul Ross

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

fmt adds an exception, from `duckdb/third_party/fmt/include/fmt/format.h`:

```LICENSE
Formatting library for C++

Copyright (c) 2012 - present, Victor Zverovich

--- Optional exception to the license ---

As an exception, if, as a result of your compiling your source code, portions
of this Software are embedded into a machine-executable object form of such
source code, you may redistribute such embedded portions in such object form
without including the above copyright and permission notices.
```

BSD-3-Clause for hyperloglog, from
`duckdb/third_party/hyperloglog/sds.hpp` (`hyperloglog.cpp` carries the same
text under `Copyright (c) 2014, Salvatore Sanfilippo`):

```LICENSE
SDSLib 2.0 -- A C dynamic strings library

Copyright (c) 2006-2015, Salvatore Sanfilippo <antirez at gmail dot com>
Copyright (c) 2015, Oran Agra
Copyright (c) 2015, Redis Labs, Inc
All rights reserved.

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are met:

  * Redistributions of source code must retain the above copyright notice,
    this list of conditions and the following disclaimer.
  * Redistributions in binary form must reproduce the above copyright
    notice, this list of conditions and the following disclaimer in the
    documentation and/or other materials provided with the distribution.
  * Neither the name of Redis nor the names of its contributors may be used
    to endorse or promote products derived from this software without
    specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS"
AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT OWNER OR CONTRIBUTORS BE
LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
POSSIBILITY OF SUCH DAMAGE.
```

BSD-2-Clause for concurrentqueue, from
`duckdb/third_party/concurrentqueue/concurrentqueue.h`:

```LICENSE
Simplified BSD license:
Copyright (c) 2013-2016, Cameron Desrochers.
All rights reserved.

Redistribution and use in source and binary forms, with or without modification,
are permitted provided that the following conditions are met:

- Redistributions of source code must retain the above copyright notice, this list of
conditions and the following disclaimer.
- Redistributions in binary form must reproduce the above copyright notice, this list of
conditions and the following disclaimer in the documentation and/or other materials
provided with the distribution.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY
EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF
MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL
THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT
OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR
TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE,
EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
```

Zlib for pdqsort, from `duckdb/third_party/pdqsort/pdqsort.h`:

```LICENSE
pdqsort.h - Pattern-defeating quicksort.

Copyright (c) 2021 Orson Peters

This software is provided 'as-is', without any express or implied warranty. In no event will the
authors be held liable for any damages arising from the use of this software.

Permission is granted to anyone to use this software for any purpose, including commercial
applications, and to alter it and redistribute it freely, subject to the following restrictions:

1. The origin of this software must not be misrepresented; you must not claim that you wrote the
   original software. If you use this software in a product, an acknowledgment in the product
   documentation would be appreciated but is not required.

2. Altered source versions must be plainly marked as such, and must not be misrepresented as
   being the original software.

3. This notice may not be removed or altered from any source distribution.
```

The Unlicense dedication for miniz, from `duckdb/third_party/miniz/miniz.cpp`:

```LICENSE
This is free and unencumbered software released into the public domain.

Anyone is free to copy, modify, publish, use, compile, sell, or
distribute this software, either in source code form or as a compiled
binary, for any purpose, commercial or non-commercial, and by any
means.

In jurisdictions that recognize copyright laws, the author or authors
of this software dedicate any and all copyright interest in the
software to the public domain. We make this dedication for the benefit
of the public at large and to the detriment of our heirs and
successors. We intend this dedication to be an overt act of
relinquishment in perpetuity of all present and future rights to this
software under copyright law.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT.
IN NO EVENT SHALL THE AUTHORS BE LIABLE FOR ANY CLAIM, DAMAGES OR
OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE,
ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR
OTHER DEALINGS IN THE SOFTWARE.

For more information, please refer to <http://unlicense.org/>
```

The MIT grant that follows it in the same file, covering miniz's ZIP layer:

```LICENSE
Copyright 2013-2014 RAD Game Tools and Valve Software
Copyright 2010-2014 Rich Geldreich and Tenacious Software LLC
All Rights Reserved.

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in
all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
THE SOFTWARE.
```

The Lucent/Plan 9 notice covering `re2/util/rune.cc` and `re2/util/utf.h`,
from `duckdb/third_party/re2/util/rune.cc`:

```LICENSE
The authors of this software are Rob Pike and Ken Thompson.
             Copyright (c) 2002 by Lucent Technologies.
Permission to use, copy, modify, and distribute this software for any
purpose without fee is hereby granted, provided that this entire notice
is included in all copies of any software which is or includes a copy
or modification of this software and in all copies of the supporting
documentation for such software.
THIS SOFTWARE IS BEING PROVIDED "AS IS", WITHOUT ANY EXPRESS OR IMPLIED
WARRANTY.  IN PARTICULAR, NEITHER THE AUTHORS NOR LUCENT TECHNOLOGIES MAKE ANY
REPRESENTATION OR WARRANTY OF ANY KIND CONCERNING THE MERCHANTABILITY
OF THIS SOFTWARE OR ITS FITNESS FOR ANY PARTICULAR PURPOSE.
```

The GNU Bison notice and its special exception, covering the generated
PostgreSQL parser in libpg_query, from
`duckdb/third_party/libpg_query/src_backend_parser_gram.cpp`:

```LICENSE
A Bison parser, made by GNU Bison 2.3.

Skeleton implementation for Bison's Yacc-like parsers in C

   Copyright (C) 1984, 1989, 1990, 2000, 2001, 2002, 2003, 2004, 2005, 2006
   Free Software Foundation, Inc.

   This program is free software; you can redistribute it and/or modify
   it under the terms of the GNU General Public License as published by
   the Free Software Foundation; either version 2, or (at your option)
   any later version.

   This program is distributed in the hope that it will be useful,
   but WITHOUT ANY WARRANTY; without even the implied warranty of
   MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
   GNU General Public License for more details.

   You should have received a copy of the GNU General Public License
   along with this program; if not, write to the Free Software
   Foundation, Inc., 51 Franklin Street, Fifth Floor,
   Boston, MA 02110-1301, USA.

   As a special exception, you may create a larger work that contains
   part or all of the Bison parser skeleton and distribute that work
   under terms of your choice, so long as that work isn't itself a
   parser generator using the skeleton or a modified version thereof
   as a parser skeleton.  Alternatively, if you modify or redistribute
   the parser skeleton itself, you may (at your option) remove this
   special exception, which will cause the skeleton and the resulting
   Bison output files to be licensed under the GNU General Public
   License without this special exception.

   This special exception was added by the Free Software Foundation in
   version 2.2 of Bison.
```

The Apache-2.0 header carried by fastpforlib, from
`duckdb/third_party/fastpforlib/bitpacking.h`:

```LICENSE
This code is released under the
Apache License Version 2.0 http://www.apache.org/licenses/.

(c) Daniel Lemire, http://fastpforlib.me/en/
```

The Apache-2.0 header carried by tdigest, from
`duckdb/third_party/tdigest/t_digest.hpp`:

```LICENSE
Licensed to Derrick R. Burns under one or more
contributor license agreements.  See the NOTICES file distributed with
this work for additional information regarding copyright ownership.
The ASF licenses this file to You under the Apache License, Version 2.0
(the "License"); you may not use this file except in compliance with
the License.  You may obtain a copy of the License at

    http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.
```

mbedtls states its terms as an SPDX identifier only; every one of its 72
license headers reads the same, for example in
`duckdb/third_party/mbedtls/library/sha256.cpp`:

```LICENSE
FIPS-180-2 compliant SHA-256 implementation

Copyright The Mbed TLS Contributors
SPDX-License-Identifier: Apache-2.0 OR GPL-2.0-or-later
```

re2, zstd and ska_sort each reference a license file that the archive does not
contain. Their headers read, respectively (`duckdb/third_party/re2/re2/re2.h`,
`duckdb/third_party/zstd/compress/zstd_compress.cpp`,
`duckdb/third_party/ska_sort/ska_sort.hpp`):

```LICENSE
Copyright 2003-2009 The RE2 Authors.  All Rights Reserved.
Use of this source code is governed by a BSD-style
license that can be found in the LICENSE file.
```

```LICENSE
Copyright (c) Meta Platforms, Inc. and affiliates.
All rights reserved.

This source code is licensed under both the BSD-style license (found in the
LICENSE file in the root directory of this source tree) and the GPLv2 (found
in the COPYING file in the root directory of this source tree).
You may select, at your option, one of the above-listed licenses.
```

```LICENSE
         Copyright Malte Skarupke 2016.
Distributed under the Boost Software License, Version 1.0.
   (See http://www.boost.org/LICENSE_1_0.txt)
```

libpg_query carries the PostgreSQL copyright lines but not the PostgreSQL
License text, for example in
`duckdb/third_party/libpg_query/src_common_keywords.cpp`:

```LICENSE
keywords.c
  lexical token lookup for key words in PostgreSQL

Portions Copyright (c) 1996-2017, PostgreSQL Global Development PGGroup
Portions Copyright (c) 1994, Regents of the University of California
```

utf8proc additionally contains derived Unicode character data. Its note, from
`duckdb/third_party/utf8proc/utf8proc.cpp`:

```LICENSE
This library contains derived data from a modified version of the
Unicode data files.

The original data files are available at
https://www.unicode.org/Public/UNIDATA/

Please notice the copyright statement in the file "utf8proc_data.c".
```

The vendored `utf8proc_data.cpp` carries no copyright statement of its own; the
Unicode terms are those published with the UNIDATA files referenced above.

## License texts the archive references but does not carry

The blocks above are everything `duckdb.tar.gz` ships. The licenses named in
them still require their terms to accompany the binary, so the canonical texts
follow. Each is the standard text for the SPDX identifier that the vendored
header names, applied to the copyright holders recorded in the table above; none
of them is a substitute for a different library's terms.

PostgreSQL License, for libpg_query (the Bison skeleton exception above covers
the generated parser in addition to this):

```LICENSE
PostgreSQL Database Management System
(formerly known as Postgres, then as Postgres95)

Portions Copyright (c) 1996-2018, PostgreSQL Global Development Group

Portions Copyright (c) 1994, The Regents of the University of California

Permission to use, copy, modify, and distribute this software and its
documentation for any purpose, without fee, and without a written agreement
is hereby granted, provided that the above copyright notice and this
paragraph and the following two paragraphs appear in all copies.

IN NO EVENT SHALL THE UNIVERSITY OF CALIFORNIA BE LIABLE TO ANY PARTY FOR
DIRECT, INDIRECT, SPECIAL, INCIDENTAL, OR CONSEQUENTIAL DAMAGES, INCLUDING
LOST PROFITS, ARISING OUT OF THE USE OF THIS SOFTWARE AND ITS
DOCUMENTATION, EVEN IF THE UNIVERSITY OF CALIFORNIA HAS BEEN ADVISED OF THE
POSSIBILITY OF SUCH DAMAGE.

THE UNIVERSITY OF CALIFORNIA SPECIFICALLY DISCLAIMS ANY WARRANTIES,
INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY
AND FITNESS FOR A PARTICULAR PURPOSE. THE SOFTWARE PROVIDED HEREUNDER IS
ON AN "AS IS" BASIS, AND THE UNIVERSITY OF CALIFORNIA HAS NO OBLIGATIONS TO
PROVIDE MAINTENANCE, SUPPORT, UPDATES, ENHANCEMENTS, OR MODIFICATIONS.
```

BSD-3-Clause, for re2 (Copyright 1999-2023 The RE2 Authors) and for zstd
(Copyright (c) Meta Platforms, Inc. and affiliates), taken under the BSD arm of
its dual license:

```LICENSE
Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are met:

1. Redistributions of source code must retain the above copyright notice,
   this list of conditions and the following disclaimer.

2. Redistributions in binary form must reproduce the above copyright notice,
   this list of conditions and the following disclaimer in the documentation
   and/or other materials provided with the distribution.

3. Neither the name of the copyright holder nor the names of its contributors
   may be used to endorse or promote products derived from this software
   without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS"
AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE
LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
POSSIBILITY OF SUCH DAMAGE.
```

Boost Software License 1.0, for ska_sort (Copyright Malte Skarupke 2016):

```LICENSE
Boost Software License - Version 1.0 - August 17th, 2003

Permission is hereby granted, free of charge, to any person or organization
obtaining a copy of the software and accompanying documentation covered by
this license (the "Software") to use, reproduce, display, distribute,
execute, and transmit the Software, and to prepare derivative works of the
Software, and to permit third-parties to whom the Software is furnished to
do so, all subject to the following:

The copyright notices in the Software and this entire statement, including
the above license grant, this restriction and the following disclaimer,
must be included in all copies of the Software, in whole or in part, and
all derivative works of the Software, unless such copies or derivative
works are solely in the form of machine-executable object code generated by
a source language processor.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE, TITLE AND NON-INFRINGEMENT. IN NO EVENT
SHALL THE COPYRIGHT HOLDERS OR ANYONE DISTRIBUTING THE SOFTWARE BE LIABLE
FOR ANY DAMAGES OR OTHER LIABILITY, WHETHER IN CONTRACT, TORT OR OTHERWISE,
ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
DEALINGS IN THE SOFTWARE.
```

Apache License 2.0, for fastpforlib, tdigest and mbedtls (taken under the
Apache arm of its dual license). pcg is dual Apache-2.0 OR MIT and is taken
under the MIT text above:

```LICENSE
                              Apache License
                        Version 2.0, January 2004
                     http://www.apache.org/licenses/

TERMS AND CONDITIONS FOR USE, REPRODUCTION, AND DISTRIBUTION

1. Definitions.

   "License" shall mean the terms and conditions for use, reproduction,
   and distribution as defined by Sections 1 through 9 of this document.

   "Licensor" shall mean the copyright owner or entity authorized by
   the copyright owner that is granting the License.

   "Legal Entity" shall mean the union of the acting entity and all
   other entities that control, are controlled by, or are under common
   control with that entity. For the purposes of this definition,
   "control" means (i) the power, direct or indirect, to cause the
   direction or management of such entity, whether by contract or
   otherwise, or (ii) ownership of fifty percent (50%) or more of the
   outstanding shares, or (iii) beneficial ownership of such entity.

   "You" (or "Your") shall mean an individual or Legal Entity
   exercising permissions granted by this License.

   "Source" form shall mean the preferred form for making modifications,
   including but not limited to software source code, documentation
   source, and configuration files.

   "Object" form shall mean any form resulting from mechanical
   transformation or translation of a Source form, including but
   not limited to compiled object code, generated documentation,
   and conversions to other media types.

   "Work" shall mean the work of authorship, whether in Source or
   Object form, made available under the License, as indicated by a
   copyright notice that is included in or attached to the work
   (an example is provided in the Appendix below).

   "Derivative Works" shall mean any work, whether in Source or Object
   form, that is based on (or derived from) the Work and for which the
   editorial revisions, annotations, elaborations, or other modifications
   represent, as a whole, an original work of authorship. For the purposes
   of this License, Derivative Works shall not include works that remain
   separable from, or merely link (or bind by name) to the interfaces of,
   the Work and Derivative Works thereof.

   "Contribution" shall mean any work of authorship, including
   the original version of the Work and any modifications or additions
   to that Work or Derivative Works thereof, that is intentionally
   submitted to Licensor for inclusion in the Work by the copyright owner
   or by an individual or Legal Entity authorized to submit on behalf of
   the copyright owner. For the purposes of this definition, "submitted"
   means any form of electronic, verbal, or written communication sent
   to the Licensor or its representatives, including but not limited to
   communication on electronic mailing lists, source code control systems,
   and issue tracking systems that are managed by, or on behalf of, the
   Licensor for the purpose of discussing and improving the Work, but
   excluding communication that is conspicuously marked or otherwise
   designated in writing by the copyright owner as "Not a Contribution."

   "Contributor" shall mean Licensor and any individual or Legal Entity
   on behalf of whom a Contribution has been received by Licensor and
   subsequently incorporated within the Work.

2. Grant of Copyright License. Subject to the terms and conditions of
   this License, each Contributor hereby grants to You a perpetual,
   worldwide, non-exclusive, no-charge, royalty-free, irrevocable
   copyright license to reproduce, prepare Derivative Works of,
   publicly display, publicly perform, sublicense, and distribute the
   Work and such Derivative Works in Source or Object form.

3. Grant of Patent License. Subject to the terms and conditions of
   this License, each Contributor hereby grants to You a perpetual,
   worldwide, non-exclusive, no-charge, royalty-free, irrevocable
   (except as stated in this section) patent license to make, have made,
   use, offer to sell, sell, import, and otherwise transfer the Work,
   where such license applies only to those patent claims licensable
   by such Contributor that are necessarily infringed by their
   Contribution(s) alone or by combination of their Contribution(s)
   with the Work to which such Contribution(s) was submitted. If You
   institute patent litigation against any entity (including a
   cross-claim or counterclaim in a lawsuit) alleging that the Work
   or a Contribution incorporated within the Work constitutes direct
   or contributory patent infringement, then any patent licenses
   granted to You under this License for that Work shall terminate
   as of the date such litigation is filed.

4. Redistribution. You may reproduce and distribute copies of the
   Work or Derivative Works thereof in any medium, with or without
   modifications, and in Source or Object form, provided that You
   meet the following conditions:

   (a) You must give any other recipients of the Work or
       Derivative Works a copy of this License; and

   (b) You must cause any modified files to carry prominent notices
       stating that You changed the files; and

   (c) You must retain, in the Source form of any Derivative Works
       that You distribute, all copyright, patent, trademark, and
       attribution notices from the Source form of the Work,
       excluding those notices that do not pertain to any part of
       the Derivative Works; and

   (d) If the Work includes a "NOTICE" text file as part of its
       distribution, then any Derivative Works that You distribute must
       include a readable copy of the attribution notices contained
       within such NOTICE file, excluding those notices that do not
       pertain to any part of the Derivative Works, in at least one
       of the following places: within a NOTICE text file distributed
       as part of the Derivative Works; within the Source form or
       documentation, if provided along with the Derivative Works; or,
       within a display generated by the Derivative Works, if and
       wherever such third-party notices normally appear. The contents
       of the NOTICE file are for informational purposes only and
       do not modify the License. You may add Your own attribution
       notices within Derivative Works that You distribute, alongside
       or as an addendum to the NOTICE text from the Work, provided
       that such additional attribution notices cannot be construed
       as modifying the License.

   You may add Your own copyright statement to Your modifications and
   may provide additional or different license terms and conditions
   for use, reproduction, or distribution of Your modifications, or
   for any such Derivative Works as a whole, provided Your use,
   reproduction, and distribution of the Work otherwise complies with
   the conditions stated in this License.

5. Submission of Contributions. Unless You explicitly state otherwise,
   any Contribution intentionally submitted for inclusion in the Work
   by You to the Licensor shall be under the terms and conditions of
   this License, without any additional terms or conditions.
   Notwithstanding the above, nothing herein shall supersede or modify
   the terms of any separate license agreement you may have executed
   with Licensor regarding such Contributions.

6. Trademarks. This License does not grant permission to use the trade
   names, trademarks, service marks, or product names of the Licensor,
   except as required for reasonable and customary use in describing the
   origin of the Work and reproducing the content of the NOTICE file.

7. Disclaimer of Warranty. Unless required by applicable law or
   agreed to in writing, Licensor provides the Work (and each
   Contributor provides its Contributions) on an "AS IS" BASIS,
   WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or
   implied, including, without limitation, any warranties or conditions
   of TITLE, NON-INFRINGEMENT, MERCHANTABILITY, or FITNESS FOR A
   PARTICULAR PURPOSE. You are solely responsible for determining the
   appropriateness of using or redistributing the Work and assume any
   risks associated with Your exercise of permissions under this License.

8. Limitation of Liability. In no event and under no legal theory,
   whether in tort (including negligence), contract, or otherwise,
   unless required by applicable law (such as deliberate and grossly
   negligent acts) or agreed to in writing, shall any Contributor be
   liable to You for damages, including any direct, indirect, special,
   incidental, or consequential damages of any character arising as a
   result of this License or out of the use or inability to use the
   Work (including but not limited to damages for loss of goodwill,
   work stoppage, computer failure or malfunction, or any and all
   other commercial damages or losses), even if such Contributor
   has been advised of the possibility of such damages.

9. Accepting Warranty or Additional Liability. While redistributing
   the Work or Derivative Works thereof, You may choose to offer,
   and charge a fee for, acceptance of support, warranty, indemnity,
   or other liability obligations and/or rights consistent with this
   License. However, in accepting such obligations, You may act only
   on Your own behalf and on Your sole responsibility, not on behalf
   of any other Contributor, and only if You agree to indemnify,
   defend, and hold each Contributor harmless for any liability
   incurred by, or claims asserted against, such Contributor by reason
   of your accepting any such warranty or additional liability.

END OF TERMS AND CONDITIONS
```
