//! Verificação de assinaturas (minisign). O registry remoto só é aceito se a
//! assinatura bater com uma das chaves públicas confiáveis embutidas no app.

use minisign_verify::{PublicKey, Signature};

use crate::error::{Error, Result};

pub trait SignatureVerifier {
    /// `signature` é o conteúdo do arquivo `.minisig`.
    fn verify(&self, data: &[u8], signature: &str) -> Result<()>;
}

pub struct MinisignVerifier {
    keys: Vec<PublicKey>,
}

impl MinisignVerifier {
    /// Chaves no formato base64 do minisign (a linha que começa com `RW`).
    /// Sem nenhuma chave, falha: nunca aceita nada "por falta de configuração".
    pub fn from_base64_keys<S: AsRef<str>>(keys: &[S]) -> Result<MinisignVerifier> {
        if keys.is_empty() {
            return Err(Error::Signature(
                "nenhuma chave pública confiável configurada".to_string(),
            ));
        }
        let mut parsed = Vec::with_capacity(keys.len());
        for key in keys {
            let key = key.as_ref().trim();
            parsed.push(PublicKey::from_base64(key).map_err(|e| {
                Error::Signature(format!("chave pública inválida '{key}': {e}"))
            })?);
        }
        Ok(MinisignVerifier { keys: parsed })
    }
}

impl SignatureVerifier for MinisignVerifier {
    fn verify(&self, data: &[u8], signature: &str) -> Result<()> {
        let signature = Signature::decode(signature)
            .map_err(|e| Error::Signature(format!("arquivo de assinatura ilegível: {e}")))?;
        // `true`: aceita também assinaturas minisign sem pré-hash (formato legado).
        if self
            .keys
            .iter()
            .any(|key| key.verify(data, &signature, true).is_ok())
        {
            Ok(())
        } else {
            Err(Error::Signature(
                "a assinatura não confere com nenhuma chave confiável".to_string(),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_to_work_without_trusted_keys() {
        let none: [&str; 0] = [];
        assert!(matches!(
            MinisignVerifier::from_base64_keys(&none),
            Err(Error::Signature(_))
        ));
    }

    #[test]
    fn rejects_malformed_public_keys() {
        assert!(MinisignVerifier::from_base64_keys(&["isto-nao-e-uma-chave"]).is_err());
    }
}
