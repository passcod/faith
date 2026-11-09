import React from "react";
import { SafeAreaView, StyleSheet, View } from "react-native";
import {
	ConnectionText,
	MochaRemoteProvider,
	StatusEmoji,
	StatusText,
} from "mocha-remote-react-native";

import { loadTests } from "./tests";

export default function App() {
	return (
		<MochaRemoteProvider tests={loadTests}>
			<SafeAreaView style={styles.container}>
				<ConnectionText style={styles.connectionText} />
				<View style={styles.statusContainer}>
					<StatusEmoji style={styles.statusEmoji} />
					<StatusText style={styles.statusText} />
				</View>
			</SafeAreaView>
		</MochaRemoteProvider>
	);
}

const styles = StyleSheet.create({
	container: { flex: 1, backgroundColor: "#fff" },
	statusContainer: { flex: 1, alignItems: "center", justifyContent: "center" },
	statusEmoji: { fontSize: 30, margin: 30, textAlign: "center" },
	statusText: { fontSize: 20, margin: 20, textAlign: "center" },
	connectionText: { textAlign: "center" },
});
