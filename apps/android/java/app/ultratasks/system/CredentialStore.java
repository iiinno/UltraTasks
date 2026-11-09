package app.ultratasks.system;

import android.content.Context;
import android.security.keystore.KeyGenParameterSpec;
import android.security.keystore.KeyProperties;
import android.util.Base64;
import java.nio.charset.StandardCharsets;
import java.security.KeyStore;
import javax.crypto.Cipher;
import javax.crypto.KeyGenerator;
import javax.crypto.SecretKey;
import javax.crypto.spec.GCMParameterSpec;

/** Ciphertext is in private preferences; only its non-exportable AES key is in AndroidKeyStore. */
public final class CredentialStore {
    private static final String ALIAS = "UltraTasks.credentials.v1";
    private static final String PREFERENCES = "encrypted_credentials";
    private static final int MAX_SECRET_BYTES = 8192;
    private static final int MAX_ENCRYPTED_CHARS = ((MAX_SECRET_BYTES + 16 + 2) / 3) * 4 + 17;
    private CredentialStore() {}

    private static void validateName(String name) {
        if (name == null || !name.matches("[A-Za-z0-9_-]{1,64}")) {
            throw new IllegalArgumentException("Invalid credential name");
        }
    }

    private static synchronized SecretKey key(boolean create) throws Exception {
        KeyStore store = KeyStore.getInstance("AndroidKeyStore");
        store.load(null);
        if (store.containsAlias(ALIAS)) return (SecretKey) store.getKey(ALIAS, null);
        if (!create) throw new IllegalStateException("Credential key is unavailable");
        KeyGenerator generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore");
        generator.init(new KeyGenParameterSpec.Builder(ALIAS,
            KeyProperties.PURPOSE_ENCRYPT | KeyProperties.PURPOSE_DECRYPT)
            .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
            .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
            .setKeySize(256).build());
        return generator.generateKey();
    }

    public static void put(Context context, String name, String secret) throws Exception {
        validateName(name);
        if (secret == null || secret.isEmpty() || secret.length() > MAX_SECRET_BYTES) {
            throw new IllegalArgumentException("Invalid credential length");
        }
        byte[] bytes = secret.getBytes(StandardCharsets.UTF_8);
        if (bytes.length > MAX_SECRET_BYTES) throw new IllegalArgumentException("Credential exceeds 8192 UTF-8 bytes");
        Cipher cipher = Cipher.getInstance("AES/GCM/NoPadding");
        cipher.init(Cipher.ENCRYPT_MODE, key(true));
        cipher.updateAAD(name.getBytes(StandardCharsets.UTF_8));
        String value = Base64.encodeToString(cipher.getIV(), Base64.NO_WRAP) + "." +
            Base64.encodeToString(cipher.doFinal(bytes), Base64.NO_WRAP);
        if (!context.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE).edit().putString(name, value).commit()) {
            throw new IllegalStateException("Credential could not be saved");
        }
    }

    public static String get(Context context, String name) throws Exception {
        validateName(name);
        String value = context.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE).getString(name, null);
        if (value == null) throw new IllegalStateException("Credential is not configured");
        if (value.length() > MAX_ENCRYPTED_CHARS) throw new IllegalStateException("Encrypted credential exceeds size limit");
        String[] parts = value.split("\\.", -1);
        if (parts.length != 2) throw new IllegalStateException("Invalid encrypted credential");
        byte[] iv = Base64.decode(parts[0], Base64.NO_WRAP);
        byte[] ciphertext = Base64.decode(parts[1], Base64.NO_WRAP);
        if (ciphertext.length < 17 || ciphertext.length > MAX_SECRET_BYTES + 16) throw new IllegalStateException("Invalid encrypted credential length");
        if (iv.length != 12) throw new IllegalStateException("Invalid encrypted credential");
        Cipher cipher = Cipher.getInstance("AES/GCM/NoPadding");
        cipher.init(Cipher.DECRYPT_MODE, key(false), new GCMParameterSpec(128, iv));
        cipher.updateAAD(name.getBytes(StandardCharsets.UTF_8));
        return new String(cipher.doFinal(ciphertext), StandardCharsets.UTF_8);
    }

    public static void remove(Context context, String name) {
        validateName(name);
        if (!context.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE).edit().remove(name).commit()) {
            throw new IllegalStateException("Credential could not be removed");
        }
    }
}
