#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct InlineBytes<const N: usize> {
    length: u16,
    bytes: [u8; N],
}

impl<const N: usize> InlineBytes<N> {
    pub(crate) const fn empty() -> Self {
        Self {
            length: 0,
            bytes: [0; N],
        }
    }

    pub(crate) fn try_from_bytes(value: &[u8]) -> Option<Self> {
        if value.len() > N {
            return None;
        }
        let length = u16::try_from(value.len()).ok()?;
        let mut bytes = [0; N];
        bytes[..value.len()].copy_from_slice(value);
        Some(Self { length, bytes })
    }

    pub(crate) fn try_nonempty(value: &[u8]) -> Option<Self> {
        (!value.is_empty())
            .then(|| Self::try_from_bytes(value))
            .flatten()
    }

    pub(crate) fn from_truncated(value: &[u8]) -> Self {
        let length = value.len().min(N).min(u16::MAX as usize);
        let mut bytes = [0; N];
        bytes[..length].copy_from_slice(&value[..length]);
        Self {
            length: length as u16,
            bytes,
        }
    }

    pub(crate) const fn len(&self) -> usize {
        self.length as usize
    }

    pub(crate) const fn len_u8(&self) -> Option<u8> {
        if self.length <= u8::MAX as u16 {
            Some(self.length as u8)
        } else {
            None
        }
    }

    pub(crate) fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.len()]
    }

    pub(crate) const fn into_array(self) -> [u8; N] {
        self.bytes
    }
}

#[cfg(test)]
mod tests {
    use super::InlineBytes;

    #[test]
    fn enforces_capacity_and_nonempty_policy_explicitly() {
        assert_eq!(
            InlineBytes::<3>::try_from_bytes(b""),
            Some(InlineBytes::empty())
        );
        assert!(InlineBytes::<3>::try_nonempty(b"").is_none());
        assert_eq!(
            InlineBytes::<3>::try_nonempty(b"abc").unwrap().as_bytes(),
            b"abc"
        );
        assert!(InlineBytes::<3>::try_from_bytes(b"abcd").is_none());
    }

    #[test]
    fn truncation_is_an_explicit_constructor() {
        assert_eq!(InlineBytes::<3>::from_truncated(b"abcd").as_bytes(), b"abc");
    }
}
