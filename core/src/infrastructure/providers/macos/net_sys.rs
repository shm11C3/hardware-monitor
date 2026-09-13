use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::net::{Ipv4Addr, Ipv6Addr};

type GatewayV4ByIfIndex = HashMap<u32, Vec<Ipv4Addr>>;
type GatewayV6ByIfIndex = HashMap<u32, Vec<Ipv6Addr>>;
type GatewaysByIfIndex = (GatewayV4ByIfIndex, GatewayV6ByIfIndex);

/// Raw interface facts collected from macOS OS APIs.
///
/// This type contains no application policy (no filtering, no gateway attachment).
#[derive(Debug, Clone)]
pub(crate) struct RawInterface {
  pub name: String,
  pub flags: u32,
  pub if_index: Option<u32>,
  pub mac_address: Option<String>,
  /// IPv4 addresses with prefix length (CIDR).
  pub ipv4: Vec<(Ipv4Addr, u32)>,
  /// All IPv6 addresses (including link-local).
  pub ipv6: Vec<Ipv6Addr>,
}

/// Collects raw interface facts via `getifaddrs`.
///
/// Gateways are retrieved separately via PF_ROUTE.
pub(crate) fn get_raw_interfaces() -> Result<Vec<RawInterface>, String> {
  Ok(
    collect_interfaces_from_getifaddrs()?
      .into_values()
      .collect(),
  )
}

struct IfAddrsGuard {
  ptr: *mut libc::ifaddrs,
}

impl Drop for IfAddrsGuard {
  fn drop(&mut self) {
    unsafe {
      if !self.ptr.is_null() {
        libc::freeifaddrs(self.ptr);
      }
    }
  }
}

///
/// Creates an RAII guard for `getifaddrs`.
///
/// # Safety
/// This function calls `getifaddrs` and returns a raw pointer that must be released
/// with `freeifaddrs`. The returned `IfAddrsGuard` guarantees that.
///
unsafe fn getifaddrs_guard() -> Result<IfAddrsGuard, String> {
  let mut ifap: *mut libc::ifaddrs = std::ptr::null_mut();
  if unsafe { libc::getifaddrs(&mut ifap) } != 0 {
    return Err(format!(
      "getifaddrs failed: {}",
      std::io::Error::last_os_error()
    ));
  }
  Ok(IfAddrsGuard { ptr: ifap })
}

/// Ensures a raw interface accumulator exists for the given interface name.
///
/// Updates the stored flags to the latest observed value.
fn ensure_interface_entry<'a>(
  map: &'a mut HashMap<String, RawInterface>,
  name: &str,
  flags: u32,
) -> &'a mut RawInterface {
  let entry = map.entry(name.to_string()).or_insert_with(|| RawInterface {
    name: name.to_string(),
    flags,
    if_index: None,
    mac_address: None,
    ipv4: vec![],
    ipv6: vec![],
  });

  // Keep the latest flags observed for this interface
  entry.flags = flags;
  entry
}

/// Applies a single `ifaddrs` entry's sockaddr facts to a `RawInterface`.
///
/// This is a low-level adapter over `getifaddrs` output:
/// - `AF_LINK`: extracts MAC address and interface index
/// - `AF_INET`: extracts IPv4 address + prefix length (from netmask)
/// - `AF_INET6`: extracts IPv6 address (including link-local)
///
/// No application policy is applied here (no filtering, no gateway attachment).
///
/// # Safety
/// Although this function is safe to call, it dereferences raw pointers stored in
/// `ifa`. The caller must ensure `ifa` originates from the active `getifaddrs`
/// list and remains valid for the duration of this call.
fn apply_sockaddr_to_raw_interface(ifa: &libc::ifaddrs, entry: &mut RawInterface) {
  if ifa.ifa_addr.is_null() {
    return;
  }

  let sa = unsafe { &*(ifa.ifa_addr as *const libc::sockaddr) };
  match sa.sa_family as i32 {
    libc::AF_LINK => {
      if let Some((mac, if_index)) =
        parse_af_link(ifa.ifa_addr as *const libc::sockaddr_dl)
      {
        entry.mac_address.get_or_insert(mac);
        entry.if_index.get_or_insert(if_index);
      }
    }
    libc::AF_INET => {
      if let Some((ip, prefix)) = parse_af_inet(
        ifa.ifa_addr as *const libc::sockaddr_in,
        ifa.ifa_netmask as *const libc::sockaddr_in,
      ) {
        entry.ipv4.push((ip, prefix));
      }
    }
    libc::AF_INET6 => {
      if let Some(ip) = parse_af_inet6(ifa.ifa_addr as *const libc::sockaddr_in6) {
        entry.ipv6.push(ip);
      }
    }
    _ => {}
  }
}

///
/// Fills missing if_index values using `if_nametoindex`.
///
/// Some interfaces may not have an AF_LINK entry in the `getifaddrs` list.
///
fn fill_missing_if_indices(map: &mut HashMap<String, RawInterface>) {
  // Fill missing if_index via if_nametoindex (some interfaces may not have AF_LINK entries)
  for (name, iface) in map.iter_mut() {
    if iface.if_index.is_some() {
      continue;
    }

    if let Ok(cname) = CString::new(name.as_str()) {
      let idx = unsafe { libc::if_nametoindex(cname.as_ptr()) };
      if idx != 0 {
        iface.if_index = Some(idx);
      }
    }
  }
}

/// Collects interfaces (name, flags, MAC, IPs, netmask) via `getifaddrs`.
///
/// Gateways are retrieved separately via PF_ROUTE.
fn collect_interfaces_from_getifaddrs() -> Result<HashMap<String, RawInterface>, String> {
  let mut map: HashMap<String, RawInterface> = HashMap::new();

  unsafe {
    let guard = getifaddrs_guard()?;
    let mut cur = guard.ptr;

    while let Some(ifa) = cur.as_ref() {
      if ifa.ifa_name.is_null() {
        cur = ifa.ifa_next;
        continue;
      }

      let name = CStr::from_ptr(ifa.ifa_name).to_string_lossy().into_owned();
      let entry = ensure_interface_entry(&mut map, &name, ifa.ifa_flags);
      apply_sockaddr_to_raw_interface(ifa, entry);

      cur = ifa.ifa_next;
    }
    // guard drops here and frees ifaddrs
  }

  fill_missing_if_indices(&mut map);
  Ok(map)
}

///
/// Parses a macOS `sockaddr_dl` to a colon-separated MAC string and if_index.
///
/// # Safety
/// `sdl_ptr` must point to a valid `sockaddr_dl` within the `getifaddrs` list.
///
fn parse_af_link(sdl_ptr: *const libc::sockaddr_dl) -> Option<(String, u32)> {
  unsafe {
    if sdl_ptr.is_null() {
      return None;
    }
    let sdl = &*sdl_ptr;
    let if_index = sdl.sdl_index as u32;
    let alen = sdl.sdl_alen as usize;
    let nlen = sdl.sdl_nlen as usize;
    if alen == 0 {
      return None;
    }

    let base = sdl.sdl_data.as_ptr() as *const u8;
    let addr_ptr = base.add(nlen);
    let bytes = std::slice::from_raw_parts(addr_ptr, alen);

    let mac = bytes
      .iter()
      .map(|b| format!("{b:02x}"))
      .collect::<Vec<_>>()
      .join(":");

    Some((mac, if_index))
  }
}

/// Parses an IPv4 address and netmask and returns (ip, prefix_length).
///
/// # Safety
/// The pointers must be valid for the duration of the call.
///
fn parse_af_inet(
  sin_ptr: *const libc::sockaddr_in,
  netmask_ptr: *const libc::sockaddr_in,
) -> Option<(Ipv4Addr, u32)> {
  unsafe {
    if sin_ptr.is_null() {
      return None;
    }

    let sin = &*sin_ptr;
    let ip = Ipv4Addr::from(u32::from_be(sin.sin_addr.s_addr));

    let prefix = if !netmask_ptr.is_null() {
      let nm = &*netmask_ptr;
      let mask = u32::from_be(nm.sin_addr.s_addr);
      mask.count_ones()
    } else {
      0
    };

    Some((ip, prefix))
  }
}

///
/// Parses an IPv6 address from `sockaddr_in6`.
///
/// # Safety
/// `sin6_ptr` must be a valid pointer to `sockaddr_in6`.
///
fn parse_af_inet6(sin6_ptr: *const libc::sockaddr_in6) -> Option<Ipv6Addr> {
  unsafe {
    if sin6_ptr.is_null() {
      return None;
    }

    let sin6 = &*sin6_ptr;
    let bytes = sin6.sin6_addr.s6_addr;
    Some(Ipv6Addr::from(bytes))
  }
}

/// Returns default gateways grouped by interface index for IPv4 and IPv6.
pub(crate) fn get_default_gateways_by_ifindex() -> Result<GatewaysByIfIndex, String> {
  let mut v4 = GatewayV4ByIfIndex::new();
  let mut v6 = GatewayV6ByIfIndex::new();

  let dump_v4 = sysctl_route_dump(libc::AF_INET)?;
  parse_default_gateways_from_route_dump_v4(&dump_v4, &mut v4);

  let dump_v6 = sysctl_route_dump(libc::AF_INET6)?;
  parse_default_gateways_from_route_dump_v6(&dump_v6, &mut v6);

  Ok((v4, v6))
}

///
/// Dumps the kernel routing table for the given address family via `sysctl`.
///
/// This uses `PF_ROUTE` + `NET_RT_DUMP` and returns the raw message buffer.
///
fn sysctl_route_dump(af: i32) -> Result<Vec<u8>, String> {
  unsafe {
    let mut mib: [libc::c_int; 6] =
      [libc::CTL_NET, libc::PF_ROUTE, 0, af, libc::NET_RT_DUMP, 0];

    let mut len: usize = 0;
    if libc::sysctl(
      mib.as_mut_ptr(),
      mib.len() as u32,
      std::ptr::null_mut(),
      &mut len,
      std::ptr::null_mut(),
      0,
    ) != 0
    {
      return Err(format!(
        "sysctl size failed: {}",
        std::io::Error::last_os_error()
      ));
    }

    let mut buf = vec![0u8; len];
    if libc::sysctl(
      mib.as_mut_ptr(),
      mib.len() as u32,
      buf.as_mut_ptr() as *mut libc::c_void,
      &mut len,
      std::ptr::null_mut(),
      0,
    ) != 0
    {
      return Err(format!(
        "sysctl data failed: {}",
        std::io::Error::last_os_error()
      ));
    }

    buf.truncate(len);
    Ok(buf)
  }
}

///
/// Parses default IPv4 gateways from a `PF_ROUTE` dump.
///
/// The result is keyed by `rtm_index` (interface index).
///
fn parse_default_gateways_from_route_dump_v4(buf: &[u8], out: &mut GatewayV4ByIfIndex) {
  let mut offset = 0usize;
  while offset + std::mem::size_of::<libc::rt_msghdr>() <= buf.len() {
    let hdr = unsafe { &*(buf.as_ptr().add(offset) as *const libc::rt_msghdr) };
    let msg_len = hdr.rtm_msglen as usize;
    if msg_len == 0 || offset + msg_len > buf.len() {
      break;
    }

    if hdr.rtm_version as i32 != libc::RTM_VERSION {
      offset += msg_len;
      continue;
    }

    let mut dst: Option<Ipv4Addr> = None;
    let mut mask: Option<Ipv4Addr> = None;
    let mut gw: Option<Ipv4Addr> = None;

    let addrs = hdr.rtm_addrs;
    let mut p = offset + std::mem::size_of::<libc::rt_msghdr>();

    for bit in [
      libc::RTA_DST,
      libc::RTA_GATEWAY,
      libc::RTA_NETMASK,
      libc::RTA_IFP,
      libc::RTA_IFA,
      libc::RTA_AUTHOR,
      libc::RTA_BRD,
    ] {
      if (addrs & bit) == 0 {
        continue;
      }
      if p + std::mem::size_of::<libc::sockaddr>() > offset + msg_len {
        break;
      }
      let sa = unsafe { &*(buf.as_ptr().add(p) as *const libc::sockaddr) };
      let sa_len = sockaddr_len(sa);

      if sa.sa_family as i32 == libc::AF_INET {
        let sin = unsafe { &*(buf.as_ptr().add(p) as *const libc::sockaddr_in) };
        let ip = Ipv4Addr::from(u32::from_be(sin.sin_addr.s_addr));
        if bit == libc::RTA_DST {
          dst = Some(ip);
        } else if bit == libc::RTA_GATEWAY {
          gw = Some(ip);
        } else if bit == libc::RTA_NETMASK {
          mask = Some(ip);
        }
      }

      p += roundup(sa_len);
    }

    let is_default = dst == Some(Ipv4Addr::UNSPECIFIED)
      && mask.unwrap_or(Ipv4Addr::UNSPECIFIED) == Ipv4Addr::UNSPECIFIED
      && (hdr.rtm_flags & libc::RTF_GATEWAY) != 0;

    if is_default && let Some(gw) = gw {
      out.entry(hdr.rtm_index as u32).or_default().push(gw);
    }

    offset += msg_len;
  }
}

///
/// Parses default IPv6 gateways from a `PF_ROUTE` dump.
///
/// The result is keyed by `rtm_index` (interface index).
///
fn parse_default_gateways_from_route_dump_v6(buf: &[u8], out: &mut GatewayV6ByIfIndex) {
  let mut offset = 0usize;
  while offset + std::mem::size_of::<libc::rt_msghdr>() <= buf.len() {
    let hdr = unsafe { &*(buf.as_ptr().add(offset) as *const libc::rt_msghdr) };
    let msg_len = hdr.rtm_msglen as usize;
    if msg_len == 0 || offset + msg_len > buf.len() {
      break;
    }

    if hdr.rtm_version as i32 != libc::RTM_VERSION {
      offset += msg_len;
      continue;
    }

    let mut dst: Option<Ipv6Addr> = None;
    let mut mask: Option<Ipv6Addr> = None;
    let mut gw: Option<Ipv6Addr> = None;

    let addrs = hdr.rtm_addrs;
    let mut p = offset + std::mem::size_of::<libc::rt_msghdr>();

    for bit in [
      libc::RTA_DST,
      libc::RTA_GATEWAY,
      libc::RTA_NETMASK,
      libc::RTA_IFP,
      libc::RTA_IFA,
      libc::RTA_AUTHOR,
      libc::RTA_BRD,
    ] {
      if (addrs & bit) == 0 {
        continue;
      }
      if p + std::mem::size_of::<libc::sockaddr>() > offset + msg_len {
        break;
      }
      let sa = unsafe { &*(buf.as_ptr().add(p) as *const libc::sockaddr) };
      let sa_len = sockaddr_len(sa);

      if sa.sa_family as i32 == libc::AF_INET6 {
        let sin6 = unsafe { &*(buf.as_ptr().add(p) as *const libc::sockaddr_in6) };
        let ip = Ipv6Addr::from(sin6.sin6_addr.s6_addr);

        if bit == libc::RTA_DST {
          dst = Some(ip);
        } else if bit == libc::RTA_GATEWAY {
          gw = Some(ip);
        } else if bit == libc::RTA_NETMASK {
          mask = Some(ip);
        }
      }

      p += roundup(sa_len);
    }

    let is_default = dst == Some(Ipv6Addr::UNSPECIFIED)
      && mask.unwrap_or(Ipv6Addr::UNSPECIFIED) == Ipv6Addr::UNSPECIFIED
      && (hdr.rtm_flags & libc::RTF_GATEWAY) != 0;

    if is_default && let Some(gw) = gw {
      out.entry(hdr.rtm_index as u32).or_default().push(gw);
    }

    offset += msg_len;
  }
}

///
/// Returns the sockaddr length used in routing messages.
///
/// On macOS, `sa_len` is present and determines the size of each sockaddr entry.
///
fn sockaddr_len(sa: &libc::sockaddr) -> usize {
  // On macOS, sa_len is available and used for routing messages.
  let len = sa.sa_len as usize;
  if len == 0 {
    std::mem::size_of::<usize>()
  } else {
    len
  }
}

/// Rounds a length up to the native word size alignment.
///
/// PF_ROUTE messages align embedded sockaddrs to `sizeof(uintptr_t)`.
fn roundup(len: usize) -> usize {
  let align = std::mem::size_of::<usize>();
  if len == 0 {
    align
  } else {
    (len + (align - 1)) & !(align - 1)
  }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
  use super::*;

  #[test]
  /// Ensures PF_ROUTE sockaddr alignment helper rounds up to word size.
  fn roundup_aligns_to_word() {
    let a = std::mem::size_of::<usize>();
    assert_eq!(roundup(1), a);
    assert_eq!(roundup(a), a);
    assert_eq!(roundup(a + 1), a * 2);
  }

  #[test]
  /// Validates IPv4 default gateway parsing from a minimal synthetic PF_ROUTE message.
  fn parse_default_gateway_v4_from_synthetic_message() {
    // Build a minimal routing message containing DST(0.0.0.0), GW(192.168.1.1), NETMASK(0.0.0.0)
    unsafe {
      let mut hdr: libc::rt_msghdr = std::mem::zeroed();
      hdr.rtm_version = libc::RTM_VERSION as u8;
      hdr.rtm_type = libc::RTM_GET as u8;
      hdr.rtm_flags = libc::RTF_GATEWAY;
      hdr.rtm_addrs = libc::RTA_DST | libc::RTA_GATEWAY | libc::RTA_NETMASK;
      hdr.rtm_index = 5;

      let mut dst: libc::sockaddr_in = std::mem::zeroed();
      dst.sin_len = std::mem::size_of::<libc::sockaddr_in>() as u8;
      dst.sin_family = libc::AF_INET as u8;
      dst.sin_addr.s_addr = u32::to_be(0);

      let mut gw: libc::sockaddr_in = std::mem::zeroed();
      gw.sin_len = std::mem::size_of::<libc::sockaddr_in>() as u8;
      gw.sin_family = libc::AF_INET as u8;
      gw.sin_addr.s_addr = u32::to_be(u32::from(Ipv4Addr::new(192, 168, 1, 1)));

      let mut nm: libc::sockaddr_in = std::mem::zeroed();
      nm.sin_len = std::mem::size_of::<libc::sockaddr_in>() as u8;
      nm.sin_family = libc::AF_INET as u8;
      nm.sin_addr.s_addr = u32::to_be(0);

      let mut bytes = Vec::<u8>::new();
      bytes.extend_from_slice(std::slice::from_raw_parts(
        &hdr as *const _ as *const u8,
        std::mem::size_of::<libc::rt_msghdr>(),
      ));
      bytes.extend_from_slice(std::slice::from_raw_parts(
        &dst as *const _ as *const u8,
        std::mem::size_of::<libc::sockaddr_in>(),
      ));
      bytes.extend_from_slice(&vec![
        0u8;
        roundup(dst.sin_len as usize) - dst.sin_len as usize
      ]);
      bytes.extend_from_slice(std::slice::from_raw_parts(
        &gw as *const _ as *const u8,
        std::mem::size_of::<libc::sockaddr_in>(),
      ));
      bytes.extend_from_slice(&vec![
        0u8;
        roundup(gw.sin_len as usize) - gw.sin_len as usize
      ]);
      bytes.extend_from_slice(std::slice::from_raw_parts(
        &nm as *const _ as *const u8,
        std::mem::size_of::<libc::sockaddr_in>(),
      ));
      bytes.extend_from_slice(&vec![
        0u8;
        roundup(nm.sin_len as usize) - nm.sin_len as usize
      ]);

      // Fix msglen
      let msglen = bytes.len() as u16;
      (bytes.as_mut_ptr() as *mut libc::rt_msghdr)
        .as_mut()
        .unwrap()
        .rtm_msglen = msglen;

      let mut out = HashMap::<u32, Vec<Ipv4Addr>>::new();
      parse_default_gateways_from_route_dump_v4(&bytes, &mut out);

      assert_eq!(out.get(&5).unwrap(), &vec![Ipv4Addr::new(192, 168, 1, 1)]);
    }
  }

  #[test]
  /// Default route without a gateway sockaddr should be ignored.
  fn parse_default_gateway_v4_ignores_default_without_gateway() {
    unsafe {
      let mut hdr: libc::rt_msghdr = std::mem::zeroed();
      hdr.rtm_version = libc::RTM_VERSION as u8;
      hdr.rtm_type = libc::RTM_GET as u8;
      hdr.rtm_flags = libc::RTF_GATEWAY;
      hdr.rtm_addrs = libc::RTA_DST | libc::RTA_NETMASK;
      hdr.rtm_index = 7;

      let mut dst: libc::sockaddr_in = std::mem::zeroed();
      dst.sin_len = std::mem::size_of::<libc::sockaddr_in>() as u8;
      dst.sin_family = libc::AF_INET as u8;
      dst.sin_addr.s_addr = u32::to_be(0);

      let mut nm: libc::sockaddr_in = std::mem::zeroed();
      nm.sin_len = std::mem::size_of::<libc::sockaddr_in>() as u8;
      nm.sin_family = libc::AF_INET as u8;
      nm.sin_addr.s_addr = u32::to_be(0);

      let mut bytes = Vec::<u8>::new();
      bytes.extend_from_slice(std::slice::from_raw_parts(
        &hdr as *const _ as *const u8,
        std::mem::size_of::<libc::rt_msghdr>(),
      ));
      bytes.extend_from_slice(std::slice::from_raw_parts(
        &dst as *const _ as *const u8,
        std::mem::size_of::<libc::sockaddr_in>(),
      ));
      bytes.extend_from_slice(&vec![
        0u8;
        roundup(dst.sin_len as usize) - dst.sin_len as usize
      ]);
      bytes.extend_from_slice(std::slice::from_raw_parts(
        &nm as *const _ as *const u8,
        std::mem::size_of::<libc::sockaddr_in>(),
      ));
      bytes.extend_from_slice(&vec![
        0u8;
        roundup(nm.sin_len as usize) - nm.sin_len as usize
      ]);

      let msglen = bytes.len() as u16;
      (bytes.as_mut_ptr() as *mut libc::rt_msghdr)
        .as_mut()
        .unwrap()
        .rtm_msglen = msglen;

      let mut out = HashMap::<u32, Vec<Ipv4Addr>>::new();
      parse_default_gateways_from_route_dump_v4(&bytes, &mut out);

      assert!(out.is_empty());
    }
  }

  #[test]
  /// Validates IPv6 default gateway parsing from a minimal synthetic PF_ROUTE message.
  fn parse_default_gateway_v6_from_synthetic_message() {
    unsafe {
      let mut hdr: libc::rt_msghdr = std::mem::zeroed();
      hdr.rtm_version = libc::RTM_VERSION as u8;
      hdr.rtm_type = libc::RTM_GET as u8;
      hdr.rtm_flags = libc::RTF_GATEWAY;
      hdr.rtm_addrs = libc::RTA_DST | libc::RTA_GATEWAY | libc::RTA_NETMASK;
      hdr.rtm_index = 9;

      let mut dst: libc::sockaddr_in6 = std::mem::zeroed();
      dst.sin6_len = std::mem::size_of::<libc::sockaddr_in6>() as u8;
      dst.sin6_family = libc::AF_INET6 as u8;
      dst.sin6_addr.s6_addr = Ipv6Addr::UNSPECIFIED.octets();

      let gw_ip = Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 1);
      let mut gw: libc::sockaddr_in6 = std::mem::zeroed();
      gw.sin6_len = std::mem::size_of::<libc::sockaddr_in6>() as u8;
      gw.sin6_family = libc::AF_INET6 as u8;
      gw.sin6_addr.s6_addr = gw_ip.octets();

      let mut nm: libc::sockaddr_in6 = std::mem::zeroed();
      nm.sin6_len = std::mem::size_of::<libc::sockaddr_in6>() as u8;
      nm.sin6_family = libc::AF_INET6 as u8;
      nm.sin6_addr.s6_addr = Ipv6Addr::UNSPECIFIED.octets();

      let mut bytes = Vec::<u8>::new();
      bytes.extend_from_slice(std::slice::from_raw_parts(
        &hdr as *const _ as *const u8,
        std::mem::size_of::<libc::rt_msghdr>(),
      ));
      bytes.extend_from_slice(std::slice::from_raw_parts(
        &dst as *const _ as *const u8,
        std::mem::size_of::<libc::sockaddr_in6>(),
      ));
      bytes.extend_from_slice(&vec![
        0u8;
        roundup(dst.sin6_len as usize)
          - dst.sin6_len as usize
      ]);
      bytes.extend_from_slice(std::slice::from_raw_parts(
        &gw as *const _ as *const u8,
        std::mem::size_of::<libc::sockaddr_in6>(),
      ));
      bytes.extend_from_slice(&vec![
        0u8;
        roundup(gw.sin6_len as usize) - gw.sin6_len as usize
      ]);
      bytes.extend_from_slice(std::slice::from_raw_parts(
        &nm as *const _ as *const u8,
        std::mem::size_of::<libc::sockaddr_in6>(),
      ));
      bytes.extend_from_slice(&vec![
        0u8;
        roundup(nm.sin6_len as usize) - nm.sin6_len as usize
      ]);

      let msglen = bytes.len() as u16;
      (bytes.as_mut_ptr() as *mut libc::rt_msghdr)
        .as_mut()
        .unwrap()
        .rtm_msglen = msglen;

      let mut out = HashMap::<u32, Vec<Ipv6Addr>>::new();
      parse_default_gateways_from_route_dump_v6(&bytes, &mut out);

      assert_eq!(out.get(&9).unwrap(), &vec![gw_ip]);
    }
  }

  #[test]
  /// Non-default IPv4 routes should be ignored.
  fn parse_default_gateway_v4_ignores_non_default_route() {
    unsafe {
      let mut hdr: libc::rt_msghdr = std::mem::zeroed();
      hdr.rtm_version = libc::RTM_VERSION as u8;
      hdr.rtm_type = libc::RTM_GET as u8;
      hdr.rtm_flags = libc::RTF_GATEWAY;
      hdr.rtm_addrs = libc::RTA_DST | libc::RTA_GATEWAY | libc::RTA_NETMASK;
      hdr.rtm_index = 11;

      let mut dst: libc::sockaddr_in = std::mem::zeroed();
      dst.sin_len = std::mem::size_of::<libc::sockaddr_in>() as u8;
      dst.sin_family = libc::AF_INET as u8;
      dst.sin_addr.s_addr = u32::to_be(u32::from(Ipv4Addr::new(10, 0, 0, 0)));

      let mut gw: libc::sockaddr_in = std::mem::zeroed();
      gw.sin_len = std::mem::size_of::<libc::sockaddr_in>() as u8;
      gw.sin_family = libc::AF_INET as u8;
      gw.sin_addr.s_addr = u32::to_be(u32::from(Ipv4Addr::new(192, 168, 1, 1)));

      let mut nm: libc::sockaddr_in = std::mem::zeroed();
      nm.sin_len = std::mem::size_of::<libc::sockaddr_in>() as u8;
      nm.sin_family = libc::AF_INET as u8;
      nm.sin_addr.s_addr = u32::to_be(u32::from(Ipv4Addr::new(255, 0, 0, 0)));

      let mut bytes = Vec::<u8>::new();
      bytes.extend_from_slice(std::slice::from_raw_parts(
        &hdr as *const _ as *const u8,
        std::mem::size_of::<libc::rt_msghdr>(),
      ));
      bytes.extend_from_slice(std::slice::from_raw_parts(
        &dst as *const _ as *const u8,
        std::mem::size_of::<libc::sockaddr_in>(),
      ));
      bytes.extend_from_slice(&vec![
        0u8;
        roundup(dst.sin_len as usize) - dst.sin_len as usize
      ]);
      bytes.extend_from_slice(std::slice::from_raw_parts(
        &gw as *const _ as *const u8,
        std::mem::size_of::<libc::sockaddr_in>(),
      ));
      bytes.extend_from_slice(&vec![
        0u8;
        roundup(gw.sin_len as usize) - gw.sin_len as usize
      ]);
      bytes.extend_from_slice(std::slice::from_raw_parts(
        &nm as *const _ as *const u8,
        std::mem::size_of::<libc::sockaddr_in>(),
      ));
      bytes.extend_from_slice(&vec![
        0u8;
        roundup(nm.sin_len as usize) - nm.sin_len as usize
      ]);

      let msglen = bytes.len() as u16;
      (bytes.as_mut_ptr() as *mut libc::rt_msghdr)
        .as_mut()
        .unwrap()
        .rtm_msglen = msglen;

      let mut out = HashMap::<u32, Vec<Ipv4Addr>>::new();
      parse_default_gateways_from_route_dump_v4(&bytes, &mut out);

      assert!(out.is_empty());
    }
  }

  #[test]
  /// Non-default IPv6 routes should be ignored.
  fn parse_default_gateway_v6_ignores_non_default_route() {
    unsafe {
      let mut hdr: libc::rt_msghdr = std::mem::zeroed();
      hdr.rtm_version = libc::RTM_VERSION as u8;
      hdr.rtm_type = libc::RTM_GET as u8;
      hdr.rtm_flags = libc::RTF_GATEWAY;
      hdr.rtm_addrs = libc::RTA_DST | libc::RTA_GATEWAY | libc::RTA_NETMASK;
      hdr.rtm_index = 13;

      let mut dst: libc::sockaddr_in6 = std::mem::zeroed();
      dst.sin6_len = std::mem::size_of::<libc::sockaddr_in6>() as u8;
      dst.sin6_family = libc::AF_INET6 as u8;
      dst.sin6_addr.s6_addr = Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 0).octets();

      let mut gw: libc::sockaddr_in6 = std::mem::zeroed();
      gw.sin6_len = std::mem::size_of::<libc::sockaddr_in6>() as u8;
      gw.sin6_family = libc::AF_INET6 as u8;
      gw.sin6_addr.s6_addr = Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 1).octets();

      let mut nm: libc::sockaddr_in6 = std::mem::zeroed();
      nm.sin6_len = std::mem::size_of::<libc::sockaddr_in6>() as u8;
      nm.sin6_family = libc::AF_INET6 as u8;
      let mut mask = [0u8; 16];
      mask[0] = 0xff;
      nm.sin6_addr.s6_addr = mask;

      let mut bytes = Vec::<u8>::new();
      bytes.extend_from_slice(std::slice::from_raw_parts(
        &hdr as *const _ as *const u8,
        std::mem::size_of::<libc::rt_msghdr>(),
      ));
      bytes.extend_from_slice(std::slice::from_raw_parts(
        &dst as *const _ as *const u8,
        std::mem::size_of::<libc::sockaddr_in6>(),
      ));
      bytes.extend_from_slice(&vec![
        0u8;
        roundup(dst.sin6_len as usize)
          - dst.sin6_len as usize
      ]);
      bytes.extend_from_slice(std::slice::from_raw_parts(
        &gw as *const _ as *const u8,
        std::mem::size_of::<libc::sockaddr_in6>(),
      ));
      bytes.extend_from_slice(&vec![
        0u8;
        roundup(gw.sin6_len as usize) - gw.sin6_len as usize
      ]);
      bytes.extend_from_slice(std::slice::from_raw_parts(
        &nm as *const _ as *const u8,
        std::mem::size_of::<libc::sockaddr_in6>(),
      ));
      bytes.extend_from_slice(&vec![
        0u8;
        roundup(nm.sin6_len as usize) - nm.sin6_len as usize
      ]);

      let msglen = bytes.len() as u16;
      (bytes.as_mut_ptr() as *mut libc::rt_msghdr)
        .as_mut()
        .unwrap()
        .rtm_msglen = msglen;

      let mut out = HashMap::<u32, Vec<Ipv6Addr>>::new();
      parse_default_gateways_from_route_dump_v6(&bytes, &mut out);

      assert!(out.is_empty());
    }
  }
}
