package io.github.chlgusrbs0.hwpconvert.parser;

import java.io.IOException;
import java.io.InputStream;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.zip.ZipEntry;
import java.util.zip.ZipInputStream;

public class HwpxPreviewTextExtractor {
    public static String extract(Path inputPath) throws IOException {
        try (InputStream fileInputStream = Files.newInputStream(inputPath);
             ZipInputStream zipInputStream = new ZipInputStream(fileInputStream)) {

            ZipEntry entry;

            while ((entry = zipInputStream.getNextEntry()) != null) {
                if (entry.getName().equals("Preview/PrvText.txt")) {
                    byte[] bytes = zipInputStream.readAllBytes();
                    zipInputStream.closeEntry();
                    return new String(bytes, StandardCharsets.UTF_8);
                }

                zipInputStream.closeEntry();
            }
        }

        return null;
    }
}