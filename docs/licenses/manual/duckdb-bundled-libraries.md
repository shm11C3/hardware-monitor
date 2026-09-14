# DuckDB bundled libraries

Covered `duckdb` version: `1.10505.0` (DuckDB 1.5.5)

Covered libduckdb-sys version: `1.10505.0`

Covered duckdb features: `bundled`

Source: `libduckdb-sys-1.10505.0/duckdb.tar.gz`, `third_party/`

This notice covers the C and C++ libraries compiled into the bundled DuckDB
engine. The published tarball contains no separate `LICENSE`, `COPYING`, or
`NOTICE` files for these directories, so the attributions below are derived
from the vendored source headers. The source archive must be re-reviewed when
the pinned `libduckdb-sys` version changes.

Which libraries are compiled depends on the features Cargo resolves for the
`duckdb` package in a `duckdb-archive` build, recorded above, so the archive
must also be re-reviewed when those change. The covered list is the resolved
set, not the dependency declaration: a feature can also be turned on by
forwarding from a `[features]` entry. Under the covered feature set the `cc` build backend compiles the
`base` section of `duckdb/manifest.json` together with the unconditionally
linked `core_functions` extension; `brotli`, `lz4`, `snappy`, `thrift` and the
`parquet` extension sources belong to the `parquet` / `json` sections, are not
built, and are therefore not listed here.

Some entries below are header-only libraries. They contribute no `.cpp` file of
their own to the manifest, but their headers sit on `base.include_dirs` and are
included by DuckDB sources that are compiled, so their code ships in the binary
and each entry names an including source.

## DuckDB

- License: MIT
- Copyright: 2021-2026 Stichting DuckDB Foundation
- Source: `libduckdb-sys/LICENSE`

## concurrentqueue

- License: BSD-2-Clause
- Copyright: 2013-2016 Cameron Desrochers
- Source header: `third_party/concurrentqueue/concurrentqueue.h`

Header-only. `src/parallel/task_scheduler.cpp` includes `concurrentqueue.h`, so
the queue implementation is compiled into the engine. The source header calls
the terms a “Simplified BSD license” and carries the two-clause redistribution
conditions reproduced below.

## fast_float

- License: MIT
- Copyright: the vendored copy states no copyright line; it credits Daniel
  Lemire and João Paulo Magalhaes, with contributions from Eugene Golushkov,
  Maksim Kita, Marcin Wojdyr, Neal Richardson, Tim Paine and Fabio Pellacini
- Source header: `third_party/fast_float/fast_float/fast_float.h`

Header-only. `src/include/duckdb/common/operator/double_cast_operator.hpp`
includes `fast_float/fast_float.h`, so the float parser is compiled into the
engine. The source header carries the MIT permission and warranty disclaimer.

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

## httplib

- License: MIT
- Copyright: 2025 Yuji Hirose
- Source header: `third_party/httplib/httplib.hpp`

Header-only. `src/main/http/http_util.cpp` includes `httplib.hpp` unless
`DUCKDB_DISABLE_BUILTIN_HTTPLIB` is defined, and the `cc` build backend in
`libduckdb-sys` defines neither that macro nor the
`DISABLE_DUCKDB_REMOTE_INSTALL` / `DUCKDB_DISABLE_EXTENSION_LOAD` macros that
would set it, so the client is compiled into the engine. The vendored copy is
cpp-httplib v0.27.0 with `std::regex` replaced by RE2. The source header
carries the copyright line and “MIT License”.

## hyperloglog

- License: BSD-3-Clause
- Copyright: 2014 Salvatore Sanfilippo
- Source header: `third_party/hyperloglog/hyperloglog.cpp`

The source header includes the BSD-3-Clause redistribution conditions and
disclaimer for the Redis HyperLogLog implementation.

## jaro_winkler

- License: MIT
- Copyright: 2022 Max Bachmann
- Source header: `third_party/jaro_winkler/jaro_winkler.hpp`

Header-only. `src/common/string_util.cpp` includes `jaro_winkler.hpp`, so the
similarity implementation is compiled into the engine. The source header states
its terms as `SPDX-License-Identifier: MIT` above the copyright line.

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
Portions Copyright (c) 1996-2017, PostgreSQL Global Development Group
Portions Copyright (c) 1994, Regents of the University of California
```

The generated Bison skeleton carries the GNU General Public License, version 2
or later, and the following special exception:

```text
As a special exception, you may create a larger work that contains part or all
of the Bison parser skeleton and distribute that work under terms of your
choice, so long as that work isn't itself a parser generator using the skeleton
or a modified version thereof as a parser skeleton.
Alternatively, if you modify or redistribute the parser skeleton itself, you
may (at your option) remove this special exception, which will cause the
skeleton and the resulting Bison output files to be licensed under the GNU
General Public License without this special exception.
This special exception was added by the Free Software Foundation in version
2.2 of Bison.
```

The applicable PostgreSQL license text is:

```text
Portions Copyright (c) 1996-2017, PostgreSQL Global Development Group
Portions Copyright (c) 1994, Regents of the University of California

Permission to use, copy, modify, and distribute this software and its
documentation for any purpose, without fee, and without a written agreement
is hereby granted, provided that the above copyright notice and this paragraph
and the following two paragraphs appear in all copies.

IN NO EVENT SHALL THE UNIVERSITY OF CALIFORNIA BE LIABLE TO ANY PARTY FOR
DIRECT, INDIRECT, SPECIAL, INCIDENTAL, OR CONSEQUENTIAL DAMAGES, INCLUDING
LOST PROFITS, ARISING OUT OF THE USE OF THIS SOFTWARE AND ITS DOCUMENTATION,
EVEN IF THE UNIVERSITY OF CALIFORNIA HAS BEEN ADVISED OF THE POSSIBILITY OF
SUCH DAMAGE.

THE UNIVERSITY OF CALIFORNIA SPECIFICALLY DISCLAIMS ANY WARRANTIES,
INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY
AND FITNESS FOR A PARTICULAR PURPOSE. THE SOFTWARE PROVIDED HEREUNDER IS
ON AN "AS IS" BASIS, AND THE UNIVERSITY OF CALIFORNIA HAS NO OBLIGATIONS TO
PROVIDE MAINTENANCE, SUPPORT, UPDATES, ENHANCEMENTS, OR MODIFICATIONS.
```

## mbedtls

- License: Apache-2.0 OR GPL-2.0-or-later. This distribution uses the
  Apache-2.0 arm of the dual license.
- Copyright: The Mbed TLS Contributors
- Source headers: `third_party/mbedtls/include/mbedtls/cipher.h` and the
  compiled files under `third_party/mbedtls/library/`

The vendored headers identify the terms with:

```text
Copyright The Mbed TLS Contributors
SPDX-License-Identifier: Apache-2.0 OR GPL-2.0-or-later
```

## miniz

- License: MIT for the compiled implementation. The vendored header also
  carries a public-domain dedication for the deflate/inflate implementation,
  and the PNG helper is attributed separately to Alex Evans as public domain.
- Copyright: 2010-2014 Rich Geldreich and Tenacious Software LLC; 2013-2014
  RAD Game Tools and Valve Software
- Source headers: `third_party/miniz/miniz.cpp` and
  `third_party/miniz/miniz.hpp`

The compiled source carries the MIT-style permission and disclaimer with the
copyright lines above. The source header's public-domain dedication is
reproduced below; this does not classify the complete miniz implementation as
public domain.

## pcg

- License: Apache-2.0 OR MIT, at the user's choice. This distribution uses the
  MIT arm of the dual license.
- Copyright: 2014-2019 Melissa O'Neill and the PCG Project contributors
- Source header: `third_party/pcg/pcg_random.hpp`

Header-only. `src/common/random_engine.cpp` includes `pcg_random.hpp`, so the
generator is compiled into the engine. The source header states
`SPDX-License-Identifier: (Apache-2.0 OR MIT)` and names both license files,
neither of which the archive vendors.

## pdqsort

- License: Zlib
- Copyright: 2021 Orson Peters
- Source header: `third_party/pdqsort/pdqsort.h`

Header-only. `src/common/sort/sorted_run_merger.cpp` includes `pdqsort.h`, so
the sort is compiled into the engine. The source header carries the full Zlib
text reproduced below.

## re2

- License: BSD-3-Clause, with additional Unicode-data terms
- Copyright: 2003-2009 The RE2 Authors; 2002 Lucent Technologies for the UTF
  helper
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

## ska_sort

- License: BSL-1.0
- Copyright: Malte Skarupke 2016
- Source header: `third_party/ska_sort/ska_sort.hpp`

Header-only. `src/common/sort/sorted_run.cpp` includes `ska_sort.hpp`, so the
radix sort is compiled into the engine. The source header references the Boost
Software License by URL; the archive does not vendor its text, so the canonical
text is reproduced below.

## skiplist

- License: MIT
- Copyright: 2015-2023 Paul Ross
- Source header: `third_party/skiplist/SkipList.h`

The source header includes the MIT permission and warranty disclaimer.

## tdigest

- License: Apache-2.0
- Copyright: licensed to Derrick R. Burns under one or more contributor license
  agreements
- Source header: `third_party/tdigest/t_digest.hpp`

Header-only. `extension/core_functions/aggregate/holistic/approximate_quantile.cpp`
includes `t_digest.hpp`, and `core_functions` is linked unconditionally by the
`cc` build backend, so the digest is compiled into the engine. The source header
carries the standard Apache-2.0 boilerplate notice.

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

## vergesort

- License: MIT
- Copyright: 2015-2020 Morwenn
- Source header: `third_party/vergesort/vergesort.h`

Header-only. `src/common/sort/sorted_run.cpp` and
`src/common/sort/sorted_run_merger.cpp` include `vergesort.h`, so the sort is
compiled into the engine. The source header carries “The MIT License (MIT)”
with the permission and warranty disclaimer.

## yyjson

- License: MIT
- Copyright: 2020 YaoYuan
- Source header: `third_party/yyjson/yyjson.cpp`

The source header includes the MIT permission and warranty disclaimer.

## zstd

- License: BSD-3-Clause OR GPL-2.0, at the user's choice. This distribution
  uses the BSD-3-Clause arm of the dual license.
- Copyright: Meta Platforms, Inc. and affiliates; 2003-2008 Yuta Mori; 2012-
  2020 Yann Collet and Facebook, Inc.; additional zstd contributors
- Source header: `third_party/zstd/common/zstd_common.cpp`

The source header states:

```text
This source code is licensed under both the BSD-style license (found in the
LICENSE file in the root directory of this source tree) and the GPLv2 (found
in the COPYING file in the root directory of this source tree).
You may select, at your option, one of the above-listed licenses.
```

The vendored `divsufsort` and `xxhash` sources additionally carry these
copyright lines:

```text
Copyright (c) 2003-2008 Yuta Mori All Rights Reserved.
Copyright (c) 2012-2020, Yann Collet, Facebook, Inc.
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

## Apache License 2.0

The Apache-2.0 components (`fastpforlib`, `tdigest`, and the selected `mbedtls`
license arm) use this text. `pcg` offers the same arm but is taken under its
MIT arm above:

```text
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

   APPENDIX: How to apply the Apache License to your work.

      To apply the Apache License to your work, attach the following
      boilerplate notice, with the fields enclosed by brackets "[]"
      replaced with your own identifying information. (Don't include
      the brackets!) The text should be enclosed in the appropriate
      comment syntax for the file format. We also recommend that a
      file or class name and description of purpose be included on the
      same "printed page" as the copyright notice for easier
      identification within third-party archives.

   Copyright [yyyy] [name of copyright owner]

   Licensed under the Apache License, Version 2.0 (the "License");
   you may not use this file except in compliance with the License.
   You may obtain a copy of the License at

       http://www.apache.org/licenses/LICENSE-2.0

   Unless required by applicable law or agreed to in writing, software
   distributed under the License is distributed on an "AS IS" BASIS,
   WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
   See the License for the specific language governing permissions and
   limitations under the License.
```

## Common BSD-3-Clause license text

The BSD-3-Clause components (`hyperloglog`, `re2`, and the selected `zstd`
license arm) use this text, with the copyright holder shown in each component
entry above:

```text
Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are met:

* Redistributions of source code must retain the above copyright notice, this
  list of conditions and the following disclaimer.

* Redistributions in binary form must reproduce the above copyright notice,
  this list of conditions and the following disclaimer in the documentation
  and/or other materials provided with the distribution.

* Neither the name of the copyright holder nor the names of its
  contributors may be used to endorse or promote products derived from
  this software without specific prior written permission.

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

## Common BSD-2-Clause license text

`concurrentqueue` is the only BSD-2-Clause component. Its vendored header
carries this text:

```text
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

## Zlib license text

`pdqsort` is the only Zlib component. Its vendored header carries this text:

```text
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

## Boost Software License 1.0 text

`ska_sort` is the only BSL-1.0 component. Its vendored header references the
license by URL rather than carrying it, so the canonical text is reproduced
here. The header itself reads:

```text
         Copyright Malte Skarupke 2016.
Distributed under the Boost Software License, Version 1.0.
   (See http://www.boost.org/LICENSE_1_0.txt)
```

```text
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

## Additional source-header notices

The following notices are included where the vendored source carries terms in
addition to the common license text.

### GNU Bison parser skeleton

`libpg_query/src_backend_parser_gram.cpp` contains the GPL-2.0-or-later
notice and the Bison exception below. The exception permits a larger work
containing the parser skeleton to be distributed under terms of the
distributor's choice; the GPL notice is retained here for completeness.

```text
Skeleton implementation for Bison's Yacc-like parsers in C

Copyright (C) 1984, 1989, 1990, 2000, 2001, 2002, 2003, 2004, 2005, 2006
Free Software Foundation, Inc.

This program is free software: you can redistribute it and/or modify
it under the terms of the GNU General Public License as published by
the Free Software Foundation, either version 2, or (at your option)
any later version.

This program is distributed in the hope that it will be useful,
but WITHOUT ANY WARRANTY; without even the implied warranty of
MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
GNU General Public License for more details.

As a special exception, you may create a larger work that contains
part or all of the Bison parser skeleton and distribute that work
under terms of your choice, so long as that work isn't itself a
parser generator using the skeleton or a modified version thereof
as a parser skeleton.
Alternatively, if you modify or redistribute the parser skeleton itself, you
may (at your option) remove this special exception, which will cause the
skeleton and the resulting Bison output files to be licensed under the GNU
General Public License without this special exception.
This special exception was added by the Free Software Foundation in
version 2.2 of Bison.
```

### miniz public-domain dedication

The `miniz` source also contains this dedication for the public-domain
portion and separately identifies the PNG writer as “Simple PNG writer function
by Alex Evans, 2011. Released into the public domain”.

```text
This is free and unencumbered software released into the public domain.

Anyone is free to copy, modify, publish, use, compile, sell, or
distribute this software, either in source code form or as a compiled
binary, for any purpose, commercial or non-commercial, and by any
means.

In jurisdictions that recognize copyright laws, the author or authors
of this software dedicate any and all copyright interest in the
software to the public domain. We make this dedication for the benefit
of the public at large and to the detriment of our heirs and
successors. We intend this dedication to be an overt act of relinquishment in
perpetuity of all present and future rights to this software under copyright
law.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT.
IN NO EVENT SHALL THE AUTHORS BE LIABLE FOR ANY CLAIM, DAMAGES OR
OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE,
ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR
OTHER DEALINGS IN THE SOFTWARE.
```

### RE2 and utf8proc Unicode notices

The RE2 UTF helper carries this additional Lucent notice:

```text
The authors of this software are Rob Pike and Ken Thompson.
Copyright (c) 2002 by Lucent Technologies.
Permission to use, copy, modify, and distribute this software for any
purpose without fee is hereby granted, provided that this entire notice
is included in all copies of any software which is or includes a copy
or modification of this software and in all copies of the supporting
documentation for such software.
THIS SOFTWARE IS BEING PROVIDED "AS IS", WITHOUT ANY EXPRESS OR IMPLIED
WARRANTY. IN PARTICULAR, NEITHER THE AUTHORS NOR LUCENT TECHNOLOGIES MAKE ANY
REPRESENTATION OR WARRANTY OF ANY KIND CONCERNING THE MERCHANTABILITY
OF THIS SOFTWARE OR ITS FITNESS FOR ANY PARTICULAR PURPOSE.
```

The utf8proc source carries this additional data notice:

```text
This library contains derived data from a modified version of the
Unicode data files.

The original data files are available at
https://www.unicode.org/Public/UNIDATA/

Please notice the copyright statement in the file "utf8proc_data.c".
```

## Refresh procedure

When `libduckdb-sys` is upgraded, or the features Cargo resolves for `duckdb`
change, extract the new `duckdb.tar.gz`, re-derive the
compiled set from `duckdb/manifest.json` (`base.cpp_files` and
`base.include_dirs`, plus the section for every enabled extension feature),
compare all compiled `third_party/` libraries and their headers with this
entry, and update the covered versions, the covered feature list, copyright
lines, license text, and extra-data notices. The covered feature list is the
`features` array of the `duckdb` node in
`cargo metadata --format-version 1 --features duckdb-archive --locked`,
sorted; `.github/scripts/check-duckdb-license-version.ts` reads the same
value. Header-only libraries are found
through `base.include_dirs`, not `base.cpp_files`; check which of their headers
compiled sources include.
Then run `cargo license --features duckdb-archive --json`,
`cargo metadata --features duckdb-archive --format-version 1`,
`node --experimental-strip-types .github/scripts/generate-licenses.ts tmp duckdb-archive`,
`node --experimental-strip-types .github/scripts/check-duckdb-license-version.ts`,
`cargo deny --manifest-path Cargo.toml --features duckdb-archive check --config
src-tauri/deny.toml licenses`, before committing the refreshed entry.
